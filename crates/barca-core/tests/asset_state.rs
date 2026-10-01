//! Asset-state dry run: for every node, would `barca get` reuse a cached
//! artifact (fresh), recompute it (stale / missing), or can't it tell?
//!
//! These drive real `commands::get` runs against a temp project, then assert
//! `asset_states` agrees with what the run actually did — the dry run must
//! compute the exact cache keys the executor computes.

use barca_core::CancellationToken;
use barca_core::asset_state::{AssetState, CacheState, StaleCause, asset_states};
use barca_core::config::ResolvedConfig;
use std::path::{Path, PathBuf};

const PIPELINE: &str = r#"
from barca import asset, task

@asset()
def a() -> dict:
    return {"n": 1}

@asset(inputs={"a": a})
def b(a: dict) -> dict:
    return {"n": a["n"] + 1}

@asset()
def c() -> dict:
    return {"n": 3}

@task(inputs={"df": b})
def validate_b(df: dict) -> None:
    assert df["n"] == 2
"#;

const FAILING: &str = r#"
from barca import asset

@asset()
def boom() -> dict:
    raise ValueError("kaboom")
"#;

const PARTITIONED: &str = r#"
from barca import asset, collect, partitions, partitions_from

@asset(partitions={"k": partitions(["x", "y"])})
def fetch(k: str) -> dict:
    return {"k": k}

@asset(inputs={"data": collect(fetch)})
def combined(data: list) -> int:
    return len(data)

@asset()
def universe() -> list:
    return ["p", "q"]

@asset(partitions={"k": partitions_from(universe)})
def dyn(k: str) -> dict:
    return {"k": k}

@asset(inputs={"rows": collect(dyn)})
def after_dyn(rows: list) -> int:
    return len(rows)
"#;

/// A temp project: one pipeline file, a python wrapper that puts the repo's
/// `python/` tree on the path (so plain `cargo test` works without a wheel),
/// and a config whose DB + artifacts live in the temp dir.
struct Project {
    dir: tempfile::TempDir,
    file: PathBuf,
    python: PathBuf,
    cfg: ResolvedConfig,
}

impl Project {
    fn new(source: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pipeline.py");
        std::fs::write(&file, source).unwrap();

        let py_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python");
        let python = dir.path().join("python");
        std::fs::write(
            &python,
            format!(
                "#!/bin/sh\nPYTHONPATH=\"{}${{PYTHONPATH:+:$PYTHONPATH}}\" exec python3 \"$@\"\n",
                py_tree.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut cfg = barca_core::config::resolve_in(None, dir.path()).unwrap();
        cfg.db_path = dir.path().join("metadata.db").display().to_string();
        cfg.artifact_root = dir.path().join("artifacts").display().to_string();

        Self {
            dir,
            file,
            python,
            cfg,
        }
    }

    fn files(&self) -> Vec<String> {
        vec![self.file.display().to_string()]
    }

    async fn get(&self, target: &str) -> Result<(), barca_core::BarcaError> {
        barca_core::commands::get(
            &self.cfg,
            Some(target),
            &self.files(),
            &self.python,
            false,
            true,
            CancellationToken::new(),
        )
        .await
        .map(|_| ())
    }

    async fn states(&self) -> Vec<AssetState> {
        asset_states(&self.cfg, &self.files()).await.unwrap()
    }

    fn rewrite(&self, from: &str, to: &str) {
        let src = std::fs::read_to_string(&self.file).unwrap();
        assert!(src.contains(from), "fixture does not contain {from:?}");
        std::fs::write(&self.file, src.replace(from, to)).unwrap();
    }

    fn db_exists(&self) -> bool {
        Path::new(&self.cfg.db_path).exists()
    }
}

fn state<'a>(states: &'a [AssetState], name: &str) -> &'a AssetState {
    states
        .iter()
        .find(|s| s.id.ends_with(&format!(":{name}")))
        .unwrap_or_else(|| panic!("no state for {name}"))
}

/// Read every file in the temp dir that belongs to the DB (main file + WAL/SHM
/// sidecars), so a test can assert a dry run left them byte-identical.
fn db_bytes(p: &Project) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(p.dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("metadata.db"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_database_everything_is_missing_and_no_db_is_created() {
    let p = Project::new(PIPELINE);
    let states = p.states().await;

    for name in ["a", "b", "c"] {
        let s = state(&states, name);
        assert_eq!(s.cache, CacheState::Missing, "{name}");
        assert!(s.last.is_none(), "{name}");
    }
    assert_eq!(state(&states, "validate_b").cache, CacheState::AlwaysRuns);

    // A dry run is read-only: it must not create the metadata DB.
    assert!(!p.db_exists(), "asset_states created the DB");
}

#[tokio::test(flavor = "multi_thread")]
async fn after_get_materialized_assets_are_fresh() {
    let p = Project::new(PIPELINE);
    p.get("b").await.unwrap();
    let states = p.states().await;

    for name in ["a", "b"] {
        let s = state(&states, name);
        assert_eq!(s.cache, CacheState::Fresh, "{name}");
        let last = s.last.as_ref().expect("last materialization");
        assert_eq!(last.status, "success");
        assert!(last.error_message.is_none());
    }
    // Never requested → never materialized.
    assert_eq!(state(&states, "c").cache, CacheState::Missing);
}

#[tokio::test(flavor = "multi_thread")]
async fn editing_an_asset_makes_it_stale_for_code_and_downstream_stale_for_upstream() {
    let p = Project::new(PIPELINE);
    p.get("b").await.unwrap();
    p.rewrite(r#"return {"n": 1}"#, r#"return {"n": 10}"#);
    let states = p.states().await;

    assert_eq!(
        state(&states, "a").cache,
        CacheState::Stale {
            cause: StaleCause::Code
        }
    );
    assert_eq!(
        state(&states, "b").cache,
        CacheState::Stale {
            cause: StaleCause::Upstream
        }
    );
    // Last materialization is still reported for stale assets.
    assert_eq!(state(&states, "b").last.as_ref().unwrap().status, "success");
}

#[tokio::test(flavor = "multi_thread")]
async fn reverting_an_edit_makes_assets_fresh_again() {
    let p = Project::new(PIPELINE);
    p.get("b").await.unwrap();
    p.rewrite(r#"return {"n": 1}"#, r#"return {"n": 10}"#);
    p.rewrite(r#"return {"n": 10}"#, r#"return {"n": 1}"#);
    let states = p.states().await;

    assert_eq!(state(&states, "a").cache, CacheState::Fresh);
    assert_eq!(state(&states, "b").cache, CacheState::Fresh);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_is_reported_as_last_materialization() {
    let p = Project::new(FAILING);
    assert!(p.get("boom").await.is_err());
    let states = p.states().await;

    let s = state(&states, "boom");
    assert_eq!(s.cache, CacheState::Missing);
    let last = s.last.as_ref().expect("failed attempt recorded");
    assert_eq!(last.status, "failed");
    assert!(
        last.error_message
            .as_deref()
            .unwrap_or("")
            .contains("kaboom"),
        "error: {:?}",
        last.error_message
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn partitioned_assets_and_dynamic_descendants() {
    let p = Project::new(PARTITIONED);
    p.get("combined").await.unwrap();
    let states = p.states().await;

    // Statically-partitioned steps are never cache-checked by the executor —
    // they always re-run — but their keys are static, so downstream still
    // cache-checks normally.
    assert_eq!(state(&states, "fetch").cache, CacheState::Partitioned);
    assert_eq!(state(&states, "combined").cache, CacheState::Fresh);

    // Dynamic partitions only exist at run time: the dry run can't know the
    // keys, so it can't know the cache keys of anything downstream either.
    assert_eq!(state(&states, "dyn").cache, CacheState::Unknown);
    assert_eq!(state(&states, "after_dyn").cache, CacheState::Unknown);
}

#[tokio::test(flavor = "multi_thread")]
async fn dry_run_leaves_an_existing_database_untouched() {
    let p = Project::new(PIPELINE);
    p.get("b").await.unwrap();
    let before = db_bytes(&p);
    assert!(!before.is_empty());

    let _ = p.states().await;

    assert_eq!(db_bytes(&p), before, "asset_states modified the DB");
}

#[test]
fn states_serialize_with_a_state_tag() {
    let fresh = serde_json::to_value(CacheState::Fresh).unwrap();
    assert_eq!(fresh, serde_json::json!({"state": "fresh"}));
    let stale = serde_json::to_value(CacheState::Stale {
        cause: StaleCause::Upstream,
    })
    .unwrap();
    assert_eq!(
        stale,
        serde_json::json!({"state": "stale", "cause": "upstream"})
    );
}
