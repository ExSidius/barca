"""Tests for the Datadog APM shim in ``barca._telemetry``.

These verify:
- ``span()`` is a no-op when the extra/env flag are off (every other
  test in the suite implicitly depends on this).
- When a fake tracer is plugged in, ``current_trace_context()`` returns
  the active trace/span ids and ``JSONFormatter`` injects them as
  ``dd.trace_id`` / ``dd.span_id`` on log records emitted inside a span.
- The high-level call sites (``reindex``, ``run_pass``, ``_execute_materialization``)
  actually open spans when the tracer is active.
"""

from __future__ import annotations

import importlib
import json
import logging
from typing import Any

import pytest

from barca import _telemetry
from barca.server.logging import JSONFormatter


class _FakeSpan:
    def __init__(self, name: str) -> None:
        self.name = name
        # 64-bit ids, same shape as ddtrace
        self.trace_id = 0xDEADBEEFCAFEBABE
        self.span_id = 0x1234567890ABCDEF
        self.tags: dict[str, str] = {}

    def set_tag(self, key: str, value: Any) -> None:
        self.tags[key] = value

    def __enter__(self) -> "_FakeSpan":
        return self

    def __exit__(self, *exc: Any) -> None:
        return None


class _FakeTracer:
    def __init__(self) -> None:
        self.opened: list[_FakeSpan] = []
        self._active: _FakeSpan | None = None

    def trace(self, name: str) -> "_FakeTraceCM":
        return _FakeTraceCM(self, name)

    def current_span(self) -> _FakeSpan | None:
        return self._active


class _FakeTraceCM:
    def __init__(self, tracer: _FakeTracer, name: str) -> None:
        self._tracer = tracer
        self._name = name
        self._span: _FakeSpan | None = None

    def __enter__(self) -> _FakeSpan:
        self._span = _FakeSpan(self._name)
        self._tracer.opened.append(self._span)
        self._tracer._active = self._span
        return self._span

    def __exit__(self, *exc: Any) -> None:
        self._tracer._active = None


@pytest.fixture
def fake_tracer(monkeypatch: pytest.MonkeyPatch):
    """Force the telemetry shim into 'enabled' mode with a fake tracer.

    Always restored via the monkeypatch + a hard module reload so other
    tests aren't poisoned by the enabled state.
    """
    tracer = _FakeTracer()
    monkeypatch.setattr(_telemetry, "_TRACER", tracer, raising=False)
    monkeypatch.setattr(_telemetry, "_ENABLED", True, raising=False)
    monkeypatch.setattr(_telemetry, "_INITIALIZED", True, raising=False)
    yield tracer
    # Reset module-level state so the no-op default returns
    importlib.reload(_telemetry)


def test_span_is_noop_when_disabled():
    """With BARCA_DATADOG_ENABLED unset, span() yields None and is cheap."""
    # Fresh module state to mimic an unconfigured process
    importlib.reload(_telemetry)
    with _telemetry.span("barca.materialize", asset_id=1) as s:
        assert s is None
    assert _telemetry.current_trace_context() == (None, None)


def test_span_records_tags_and_exposes_trace_context(fake_tracer: _FakeTracer):
    """Inside a span, current_trace_context() returns stringified ids."""
    with _telemetry.span("barca.run_pass", repo_root="/tmp/x") as s:
        assert s is not None
        assert s.tags == {"repo_root": "/tmp/x"}
        trace_id, span_id = _telemetry.current_trace_context()
        assert trace_id == str(0xDEADBEEFCAFEBABE)
        assert span_id == str(0x1234567890ABCDEF)
    # Context is gone once the span closes
    assert _telemetry.current_trace_context() == (None, None)


def test_jsonformatter_emits_dd_fields_inside_span(fake_tracer: _FakeTracer):
    """A log record emitted inside a span carries dd.trace_id / dd.span_id."""
    formatter = JSONFormatter()
    record = logging.LogRecord(
        name="barca.test",
        level=logging.INFO,
        pathname=__file__,
        lineno=1,
        msg="hello",
        args=(),
        exc_info=None,
    )

    # Outside any span: keys omitted
    payload = json.loads(formatter.format(record))
    assert "dd.trace_id" not in payload
    assert "dd.span_id" not in payload

    # Inside a span: keys present and stringified
    with _telemetry.span("barca.materialize", asset_slug="foo"):
        payload = json.loads(formatter.format(record))
    assert payload["dd.trace_id"] == str(0xDEADBEEFCAFEBABE)
    assert payload["dd.span_id"] == str(0x1234567890ABCDEF)


def test_call_sites_open_named_spans(fake_tracer: _FakeTracer, tmp_path):
    """run_pass / reindex open the documented spans when the tracer is on."""
    import textwrap
    import sys as _sys

    project_dir = tmp_path / "telproj"
    project_dir.mkdir()
    mod_dir = project_dir / "telmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def producer():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["telmod.assets"]
        """)
    )

    # Same cleanup pattern other tests use
    for k in [m for m in _sys.modules if m == "telmod" or m.startswith("telmod.")]:
        del _sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()

    _sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass
        from barca._store import MetadataStore

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        run_pass(store, project_dir)
    finally:
        _sys.path.remove(str(project_dir))
        for k in [m for m in _sys.modules if m == "telmod" or m.startswith("telmod.")]:
            del _sys.modules[k]
        clear_caches()

    names = [s.name for s in fake_tracer.opened]
    assert "barca.run_pass" in names
    assert "barca.reindex" in names
    assert "barca.materialize" in names
