//! Workflow 4: Sequential ML pipeline (iris dataset).
//! Tests a 4-asset dependency chain: raw_data → train_test_split → trained_model → evaluation.
//! Validates dependency ordering, artifact correctness, and caching.

mod helpers;

use serial_test::serial;
use std::path::PathBuf;

fn iris_dir() -> PathBuf {
    helpers::fixture_dir_for("iris_pipeline")
}

fn barca(args: &[&str]) -> assert_cmd::Command {
    helpers::barca_in(iris_dir(), args)
}

fn reset() {
    helpers::reset_in(&iris_dir());
}

fn reindex() -> String {
    helpers::reindex_in(&iris_dir())
}

fn find_asset_id(name: &str) -> i64 {
    helpers::find_asset_id_in(&iris_dir(), name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_reindex_discovers_all_pipeline_assets() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    let output = reindex();
    assert!(output.contains("raw_data"), "should discover raw_data");
    assert!(output.contains("train_test_split"), "should discover train_test_split");
    assert!(output.contains("trained_model"), "should discover trained_model");
    assert!(output.contains("evaluation"), "should discover evaluation");
}

#[test]
#[serial]
fn test_full_pipeline_refresh_from_leaf() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    reindex();

    let eval_id = find_asset_id("evaluation");

    // Refreshing the leaf should cascade and materialize all 4 assets
    let output = barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("success"), "pipeline should complete successfully");

    // All 4 assets should now be materialized
    let barcafiles = iris_dir().join(".barcafiles");
    assert!(barcafiles.exists(), ".barcafiles/ should exist");

    let artifact_count = std::fs::read_dir(&barcafiles).unwrap().count();
    assert_eq!(artifact_count, 4, "should have 4 asset artifact directories");
}

#[test]
#[serial]
fn test_evaluation_artifact_has_correct_structure() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    reindex();

    let eval_id = find_asset_id("evaluation");
    barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    // Find and read the evaluation artifact
    let barcafiles = iris_dir().join(".barcafiles");
    let eval_value = find_value_json(&barcafiles, "evaluation");
    let value: serde_json::Value = serde_json::from_str(&eval_value).expect("evaluation artifact should be valid JSON");

    // Check required fields
    assert!(value.get("test_accuracy").is_some(), "should have test_accuracy");
    assert!(value.get("train_accuracy").is_some(), "should have train_accuracy");
    assert!(value.get("feature_importances").is_some(), "should have feature_importances");
    assert!(value.get("classification_report").is_some(), "should have classification_report");

    // Accuracy should be high (iris is easy)
    let test_acc = value["test_accuracy"].as_f64().unwrap();
    assert!(test_acc > 0.9, "test accuracy should be > 0.9, got {}", test_acc);

    // Feature importances should have 4 entries (iris has 4 features)
    let importances = value["feature_importances"].as_object().unwrap();
    assert_eq!(importances.len(), 4, "should have 4 feature importances");
    assert!(importances.contains_key("petal length (cm)"), "should have petal length");
    assert!(importances.contains_key("petal width (cm)"), "should have petal width");
}

#[test]
#[serial]
fn test_intermediate_artifacts_are_correct() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    reindex();

    let eval_id = find_asset_id("evaluation");
    barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    let barcafiles = iris_dir().join(".barcafiles");

    // raw_data should have features, targets, feature_names, target_names
    let raw = serde_json::from_str::<serde_json::Value>(&find_value_json(&barcafiles, "raw-data")).unwrap();
    assert_eq!(raw["features"].as_array().unwrap().len(), 150, "iris has 150 samples");
    assert_eq!(raw["target_names"].as_array().unwrap().len(), 3, "iris has 3 classes");

    // train_test_split should have 120 train, 30 test (80/20 split)
    let split = serde_json::from_str::<serde_json::Value>(&find_value_json(&barcafiles, "train-test-split")).unwrap();
    assert_eq!(split["X_train"].as_array().unwrap().len(), 120, "should have 120 train samples");
    assert_eq!(split["X_test"].as_array().unwrap().len(), 30, "should have 30 test samples");

    // trained_model should have predictions matching test set size
    let model = serde_json::from_str::<serde_json::Value>(&find_value_json(&barcafiles, "trained-model")).unwrap();
    assert_eq!(model["predictions"].as_array().unwrap().len(), 30, "should have 30 predictions");
    assert_eq!(model["n_estimators"].as_i64().unwrap(), 50, "should use 50 estimators");
}

#[test]
#[serial]
fn test_pipeline_second_run_is_cached() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    reindex();

    let eval_id = find_asset_id("evaluation");

    // First run
    barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    // Second run should be fast (cached)
    let start = std::time::Instant::now();
    let output = barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let elapsed = start.elapsed();
    let output = String::from_utf8(output).unwrap();

    assert!(
        output.contains("already fresh") || elapsed.as_secs() < 5,
        "second pipeline run should be cached, took {:.1}s",
        elapsed.as_secs_f64()
    );
}

#[test]
#[serial]
fn test_refreshing_midpoint_materializes_upstream_only() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();
    reindex();

    // Refresh train_test_split (midpoint) — should only materialize raw_data + train_test_split
    let split_id = find_asset_id("train_test_split");
    barca(&["assets", "refresh", &split_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let barcafiles = iris_dir().join(".barcafiles");
    let dirs: Vec<String> = std::fs::read_dir(&barcafiles)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    // Should have raw_data and train_test_split, but NOT trained_model or evaluation
    assert!(dirs.iter().any(|d| d.contains("raw-data")), "should have raw_data artifact");
    assert!(dirs.iter().any(|d| d.contains("train-test-split")), "should have train_test_split artifact");
    assert!(!dirs.iter().any(|d| d.contains("trained-model")), "should NOT have trained_model artifact");
    assert!(!dirs.iter().any(|d| d.contains("evaluation")), "should NOT have evaluation artifact");
}

// ---------------------------------------------------------------------------
// Far-off dependency change tests
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_helper_module_change_invalidates_pipeline() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();

    // Create a helper module that raw_data "depends on" via codebase hash
    let helpers_path = iris_dir().join("iris_project").join("constants.py");
    std::fs::write(&helpers_path, "N_ESTIMATORS = 50\n").unwrap();

    reindex();
    let eval_id = find_asset_id("evaluation");

    // First run — full pipeline
    barca(&["assets", "refresh", &eval_id.to_string()])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    // Read the definition hash from `assets show`
    let show_1 = String::from_utf8(
        barca(&["assets", "show", &eval_id.to_string()])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let def_hash_1 = extract_definition_hash(&show_1);

    // Change the helper module
    std::fs::write(&helpers_path, "N_ESTIMATORS = 100\n").unwrap();

    // Reindex should pick up the codebase change
    reindex();

    // Definition hash should have changed
    let show_2 = String::from_utf8(
        barca(&["assets", "show", &eval_id.to_string()])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let def_hash_2 = extract_definition_hash(&show_2);

    assert_ne!(
        def_hash_1, def_hash_2,
        "definition hash should change when a helper module is modified"
    );

    // Refresh should actually run (not cache hit) since definition changed
    let output = String::from_utf8(
        barca(&["assets", "refresh", &eval_id.to_string()])
            .timeout(std::time::Duration::from_secs(60))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(output.contains("success"), "should re-materialize after helper change");

    // Clean up the helper file
    std::fs::remove_file(&helpers_path).ok();
}

#[test]
#[serial]
fn test_version_history_after_helper_change() {
    helpers::ensure_python_ready_in(&iris_dir());
    reset();

    let helpers_path = iris_dir().join("iris_project").join("version.py");
    std::fs::write(&helpers_path, "V = 1\n").unwrap();

    reindex();
    let raw_id = find_asset_id("raw_data");

    // First materialization
    barca(&["assets", "refresh", &raw_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Check jobs list has 1 job
    let jobs_1 = String::from_utf8(
        barca(&["jobs", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let success_count_1 = jobs_1.matches("success").count();

    // Change helper, reindex, rematerialize
    std::fs::write(&helpers_path, "V = 2\n").unwrap();
    reindex();
    barca(&["assets", "refresh", &raw_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Should now have more jobs in history
    let jobs_2 = String::from_utf8(
        barca(&["jobs", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let success_count_2 = jobs_2.matches("success").count();

    assert!(
        success_count_2 > success_count_1,
        "should have more successful jobs after helper change (was {}, now {})",
        success_count_1,
        success_count_2,
    );

    // Clean up
    std::fs::remove_file(&helpers_path).ok();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the definition hash from `assets show` output.
fn extract_definition_hash(output: &str) -> String {
    for line in output.lines() {
        if line.contains("Definition hash:") {
            return line.split(':').last().unwrap_or("").trim().to_string();
        }
    }
    panic!("Definition hash not found in output:\n{}", output);
}

/// Find the value.json file for an asset whose directory name contains `slug`.
fn find_value_json(barcafiles: &std::path::Path, slug: &str) -> String {
    for entry in std::fs::read_dir(barcafiles).unwrap().filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().contains(slug) {
            // Walk into the definition hash subdirectory
            for sub in std::fs::read_dir(entry.path()).unwrap().filter_map(|e| e.ok()) {
                let value_path = sub.path().join("value.json");
                if value_path.exists() {
                    return std::fs::read_to_string(&value_path).unwrap();
                }
            }
        }
    }
    panic!("value.json not found for asset containing '{}'", slug);
}
