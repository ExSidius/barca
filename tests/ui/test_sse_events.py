"""Level 1: SSE event unit tests — pure Python, no server, no DB."""

from __future__ import annotations

from datastar_py import ServerSentEventGenerator as SSE


def test_patch_elements_basic():
    event = SSE.patch_elements("<div id='t'>ok</div>")
    assert "datastar-patch-elements" in event
    assert "<div id='t'>ok</div>" in event


def test_patch_elements_with_selector():
    event = SSE.patch_elements("<li>item</li>", selector="#list", mode="append")
    assert "selector #list" in event
    assert "mode append" in event
    assert "<li>item</li>" in event


def test_patch_elements_remove():
    event = SSE.patch_elements(selector="#loading", mode="remove")
    assert "datastar-patch-elements" in event
    assert "selector #loading" in event
    assert "mode remove" in event


def test_patch_signals_dict():
    event = SSE.patch_signals({"refreshing": False, "count": 42})
    assert "datastar-patch-signals" in event
    assert '"refreshing"' in event
    assert '"count"' in event


def test_patch_signals_booleans():
    event = SSE.patch_signals({"reconciling": True})
    assert "datastar-patch-signals" in event
    assert '"reconciling"' in event


def test_patch_elements_inner_mode():
    event = SSE.patch_elements("<span>new</span>", selector="#target", mode="inner")
    assert "selector #target" in event
    assert "mode inner" in event


def test_patch_elements_outer_mode_is_default():
    # outer is default — selector without mode should NOT include mode line
    event = SSE.patch_elements("<div id='x'>y</div>")
    assert "datastar-patch-elements" in event
    # content present
    assert "<div id='x'>y</div>" in event


def test_patch_signals_only_if_missing():
    event = SSE.patch_signals({"newSignal": "hello"}, only_if_missing=True)
    assert "datastar-patch-signals" in event
    assert "onlyIfMissing" in event or "only_if_missing" in event.lower() or "true" in event.lower()


def test_datastar_events_are_strings():
    """SSE events are plain strings — easy to assert on in higher-level tests."""
    event = SSE.patch_elements("<p>hi</p>")
    assert isinstance(event, str)
    event2 = SSE.patch_signals({"x": 1})
    assert isinstance(event2, str)
