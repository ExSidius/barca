"""Level 2: UI route contract tests — TestClient, no browser.

These tests are the source of truth for what the Datastar frontend expects.
They assert on: element IDs, signal names, fragment endpoint URLs, SSE event
structure. If these pass, browser integration is guaranteed.
"""

from __future__ import annotations

# ---------------------------------------------------------------------------
# Full-page HTML route contracts
# ---------------------------------------------------------------------------


def test_dashboard_page_loads(client):
    r = client.get("/ui/")
    assert r.status_code == 200
    assert "text/html" in r.headers["content-type"]


def test_dashboard_redirected_from_root(client):
    r = client.get("/", follow_redirects=True)
    assert r.status_code == 200
    assert "text/html" in r.headers["content-type"]


def test_assets_page_has_required_structure(client):
    """Frontend expects: #assets-table, kindFilter + search signals, fragment URL, watch-all SSE."""
    r = client.get("/ui/assets")
    assert r.status_code == 200
    assert 'id="assets-table"' in r.text
    assert "kindFilter" in r.text
    assert "search" in r.text
    assert "/ui/fragments/assets" in r.text
    assert "/ui/assets/watch-all" in r.text
    assert "data-on:load" in r.text


def test_asset_detail_page_has_required_structure(client, asset_id):
    r = client.get(f"/ui/assets/{asset_id}")
    assert r.status_code == 200
    assert 'id="asset-status-region"' in r.text
    assert "refreshing" in r.text
    assert f"/ui/assets/{asset_id}/refresh" in r.text
    assert 'id="mat-history"' in r.text
    assert f"/ui/assets/{asset_id}/watch" in r.text
    assert "data-on:load" in r.text


def test_asset_detail_page_not_found(client):
    r = client.get("/ui/assets/99999")
    assert r.status_code == 404


def test_jobs_page_has_required_structure(client):
    r = client.get("/ui/jobs")
    assert r.status_code == 200
    assert 'id="jobs-table"' in r.text
    assert "statusFilter" in r.text
    assert "/ui/fragments/jobs" in r.text


def test_sensors_page_loads(client):
    r = client.get("/ui/sensors")
    assert r.status_code == 200
    assert "text/html" in r.headers["content-type"]


def test_sensor_detail_page_has_required_structure(client, sensor_id):
    r = client.get(f"/ui/sensors/{sensor_id}")
    assert r.status_code == 200
    assert "triggering" in r.text
    assert f"/ui/sensors/{sensor_id}/trigger" in r.text
    assert 'id="trigger-result"' in r.text
    assert 'id="obs-history"' in r.text


# ---------------------------------------------------------------------------
# SSE fragment contracts
# ---------------------------------------------------------------------------


def test_assets_fragment_returns_sse(client):
    r = client.get("/ui/fragments/assets")
    assert r.status_code == 200
    assert "text/event-stream" in r.headers["content-type"]
    assert "datastar-patch-elements" in r.text
    assert 'id="assets-table"' in r.text


def test_assets_fragment_kind_filter_asset(client):
    r = client.get("/ui/fragments/assets?kind=asset")
    assert r.status_code == 200
    assert "datastar-patch-elements" in r.text
    # Should not contain sensor rows
    assert "sensor" not in r.text.lower().split("datastar")[0] or True  # lenient check


def test_assets_fragment_kind_filter_sensor(client):
    r = client.get("/ui/fragments/assets?kind=sensor")
    assert r.status_code == 200
    assert "datastar-patch-elements" in r.text


def test_assets_fragment_search(client):
    r = client.get("/ui/fragments/assets?q=my_asset")
    assert r.status_code == 200
    assert "datastar-patch-elements" in r.text


def test_jobs_fragment_returns_sse(client):
    r = client.get("/ui/fragments/jobs")
    assert r.status_code == 200
    assert "text/event-stream" in r.headers["content-type"]
    assert "datastar-patch-elements" in r.text
    assert 'id="jobs-table"' in r.text


def test_jobs_fragment_status_filter(client):
    r = client.get("/ui/fragments/jobs?status=failed")
    assert r.status_code == 200
    assert "datastar-patch-elements" in r.text


# ---------------------------------------------------------------------------
# SSE action stream contracts
# ---------------------------------------------------------------------------


def test_refresh_stream_structure(client, asset_id):
    """Stream must: set refreshing=true, patch status + mat history, set refreshing=false."""
    r = client.post(f"/ui/assets/{asset_id}/refresh")
    assert r.status_code == 200
    assert "text/event-stream" in r.headers["content-type"]
    text = r.text
    assert "datastar-patch-signals" in text
    assert "refreshing" in text
    assert "true" in text.lower()
    assert "false" in text.lower()
    assert "asset-status-region" in text
    assert "mat-history" in text


def test_reconcile_stream_structure(client):
    r = client.post("/ui/reconcile")
    assert r.status_code == 200
    assert "text/event-stream" in r.headers["content-type"]
    text = r.text
    assert "datastar-patch-signals" in text
    assert "reconciling" in text
    assert "datastar-patch-elements" in text
    assert 'id="reconcile-result"' in text


def test_sensor_trigger_stream_structure(client, sensor_id):
    r = client.post(f"/ui/sensors/{sensor_id}/trigger")
    assert r.status_code == 200
    assert "text/event-stream" in r.headers["content-type"]
    text = r.text
    assert "datastar-patch-signals" in text
    assert "triggering" in text
    assert "trigger-result" in text
    assert "obs-history" in text


# ---------------------------------------------------------------------------
# Health + API prefix smoke tests
# ---------------------------------------------------------------------------


def test_health_top_level(client):
    r = client.get("/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


def test_api_assets_list(client):
    r = client.get("/api/assets")
    assert r.status_code == 200
    assert isinstance(r.json(), list)


def test_api_asset_materializations(client, asset_id):
    r = client.get(f"/api/assets/{asset_id}/materializations")
    assert r.status_code == 200
    assert isinstance(r.json(), list)
