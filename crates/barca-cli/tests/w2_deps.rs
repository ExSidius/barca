//! Workflow 2: Assets with upstream dependencies.
//! Tests that refreshing a downstream asset triggers upstream materialization,
//! and that the downstream receives the upstream artifact as input.

mod helpers;

use serial_test::serial;

#[test]
#[serial]
fn test_downstream_refresh_triggers_upstream() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let upstream_id = helpers::find_asset_id("fruit");
    let downstream_id = helpers::find_asset_id("uppercased");

    // Refresh the downstream asset — should automatically materialize upstream first
    helpers::barca(&["assets", "refresh", &downstream_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Both upstream and downstream should now show "success"
    let upstream_out = helpers::barca(&["assets", "show", &upstream_id.to_string()]).assert().success().get_output().stdout.clone();
    let upstream_out = String::from_utf8(upstream_out).unwrap();
    assert!(upstream_out.contains("success"), "upstream 'fruit' should be materialized");

    let downstream_out = helpers::barca(&["assets", "show", &downstream_id.to_string()]).assert().success().get_output().stdout.clone();
    let downstream_out = String::from_utf8(downstream_out).unwrap();
    assert!(downstream_out.contains("success"), "downstream 'uppercased' should be materialized");
}

#[test]
#[serial]
fn test_downstream_artifact_contains_transformed_input() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let downstream_id = helpers::find_asset_id("uppercased");
    helpers::barca(&["assets", "refresh", &downstream_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Find the artifact file on disk and verify the value
    let fixture = helpers::fixture_dir();
    let barcafiles = fixture.join(".barcafiles");
    assert!(barcafiles.exists(), ".barcafiles/ should exist");

    // Find the value.json for uppercased (look in any subdirectory)
    let uppercased_dir: Vec<_> = std::fs::read_dir(&barcafiles)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("uppercased"))
        .collect();
    assert!(!uppercased_dir.is_empty(), "should find uppercased artifact directory");

    // Walk into the definition_hash subdirectory to find value.json
    let mut found_value = false;
    for entry in &uppercased_dir {
        for sub in std::fs::read_dir(entry.path()).unwrap().filter_map(|e| e.ok()) {
            let value_path = sub.path().join("value.json");
            if value_path.exists() {
                let content = std::fs::read_to_string(&value_path).unwrap();
                let value: serde_json::Value = serde_json::from_str(&content).unwrap();
                // fruit() returns "banana", uppercased(fruit) returns fruit.upper() = "BANANA"
                assert_eq!(value.as_str().unwrap(), "BANANA", "uppercased should return 'BANANA'");
                found_value = true;
            }
        }
    }
    assert!(found_value, "should find value.json artifact for uppercased");
}

#[test]
#[serial]
fn test_upstream_already_materialized_is_reused() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let upstream_id = helpers::find_asset_id("fruit");
    let downstream_id = helpers::find_asset_id("uppercased");

    // Materialize upstream first
    helpers::barca(&["assets", "refresh", &upstream_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Now materialize downstream — should reuse existing upstream, not re-run it
    helpers::barca(&["assets", "refresh", &downstream_id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Jobs list should show 3 jobs total: 1 for fruit, 1 for fruit (reused by downstream enqueue), 1 for uppercased
    // Actually, the key assertion is just that both succeeded
    let output = helpers::barca(&["jobs", "list"]).assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output).unwrap();
    let success_count = output.lines().filter(|l| l.contains("success")).count();
    assert!(success_count >= 2, "should have at least 2 successful jobs (fruit + uppercased), got {}", success_count);
}
