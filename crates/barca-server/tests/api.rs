//! Smoke tests for the HTTP API. These drive the router directly via
//! `tower::ServiceExt::oneshot` (no socket bind). They cover the pure
//! static-analysis endpoints (/health, /plan, /assets) and the 404 path, which
//! do not require a Python execution environment.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use barca_server::{ServeConfig, app};
use std::io::Write;
use tower::ServiceExt; // for `oneshot`

const FIXTURE: &str = r#"
from barca import asset

@asset()
def first() -> dict:
    return {"n": 1}

@asset(inputs={"first": first})
def second(first: dict) -> dict:
    return {"n": first["n"] + 1}
"#;

/// Write the fixture module to a temp dir and build a config pointing at it.
fn fixture_config(dir: &std::path::Path) -> ServeConfig {
    let path = dir.join("pipeline.py");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(FIXTURE.as_bytes()).unwrap();
    ServeConfig {
        files: vec![path.display().to_string()],
        host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port: 0,
        watch: false,
        python: barca_core::commands::find_python(),
    }
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_reports_ok_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(fixture_config(dir.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn assets_lists_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(fixture_config(dir.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let assets = json.as_array().expect("array of assets");
    assert_eq!(assets.len(), 2);
    let ids: Vec<&str> = assets.iter().filter_map(|a| a["id"].as_str()).collect();
    assert!(ids.iter().any(|id| id.ends_with(":first")));
    assert!(ids.iter().any(|id| id.ends_with(":second")));
    // `second` depends on `first`.
    let second = assets
        .iter()
        .find(|a| a["id"].as_str().unwrap().ends_with(":second"))
        .unwrap();
    assert_eq!(second["inputs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn plan_returns_phases() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(fixture_config(dir.path()));
    let resp = app
        .oneshot(Request::builder().uri("/plan").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["total_steps"], 2);
    assert!(json["phases"].is_array());
}

#[tokio::test]
async fn unknown_asset_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(fixture_config(dir.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets/does_not_exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn status_for_unknown_run_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(fixture_config(dir.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/status/deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
