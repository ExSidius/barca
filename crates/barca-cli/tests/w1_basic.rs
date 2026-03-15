//! Workflow 1: Single assets with no inputs.
//! Tests reindex, list, show, refresh, and reset against a real Python project.

mod helpers;

use serial_test::serial;

#[test]
#[serial]
fn test_reindex_discovers_all_assets() {
    helpers::ensure_python_ready();
    helpers::reset();
    let output = helpers::reindex();
    assert!(output.contains("hello_world"), "should discover hello_world");
    assert!(output.contains("greeting"), "should discover greeting");
    assert!(output.contains("slow_computation"), "should discover slow_computation");
    assert!(output.contains("fruit"), "should discover fruit");
    assert!(output.contains("uppercased"), "should discover uppercased");
    assert!(output.contains("fetch_prices"), "should discover fetch_prices");
    assert!(output.contains("wide_asset"), "should discover wide_asset");
    assert!(output.contains("bare_asset"), "should discover bare_asset");
}

#[test]
#[serial]
fn test_bare_asset_decorator_is_indexed() {
    // Regression test: @asset (no parentheses) must be indexed the same as @asset().
    helpers::ensure_python_ready();
    helpers::reset();
    let output = helpers::reindex();
    assert!(
        output.contains("bare_asset"),
        "bare @asset decorator (no parentheses) should be discovered during reindex; got:\n{}",
        output
    );
    // Confirm it can be shown and refreshed like any other asset.
    let id = helpers::find_asset_id("bare_asset");
    let show = helpers::barca(&["assets", "show", &id.to_string()]).assert().success().get_output().stdout.clone();
    let show = String::from_utf8(show).unwrap();
    assert!(show.contains("bare_asset"), "show should display bare_asset");
}

#[test]
#[serial]
fn test_assets_list_shows_status() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();
    let output = helpers::assets_list();
    // All assets should show "never run" initially
    assert!(output.contains("never run"), "fresh assets should show 'never run'");
}

#[test]
#[serial]
fn test_assets_show_displays_detail() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();
    let id = helpers::find_asset_id("hello_world");
    let output = helpers::barca(&["assets", "show", &id.to_string()]).assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("hello_world"), "show output should contain function name");
    assert!(output.contains("example_project.assets"), "show output should contain module path");
    assert!(output.contains("Definition hash:"), "show output should contain definition hash");
}

#[test]
#[serial]
fn test_refresh_simple_asset() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();
    let id = helpers::find_asset_id("hello_world");
    // Refresh should succeed and show "success" in the detail output
    let output = helpers::barca(&["assets", "refresh", &id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("success"), "refresh output should show success");

    // Verify artifact was created on disk
    let fixture = helpers::fixture_dir();
    let barcafiles = fixture.join(".barcafiles");
    assert!(barcafiles.exists(), ".barcafiles/ should exist after refresh");
}

#[test]
#[serial]
fn test_refresh_is_cached_on_second_run() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();
    let id = helpers::find_asset_id("greeting");

    // First refresh
    helpers::barca(&["assets", "refresh", &id.to_string()]).timeout(std::time::Duration::from_secs(30)).assert().success();

    // Second refresh should be instant (cached)
    let output = helpers::barca(&["assets", "refresh", &id.to_string()])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("already fresh") || output.contains("success"), "second refresh should be cached or succeed");
}

#[test]
#[serial]
fn test_reset_clears_state() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();
    let id = helpers::find_asset_id("hello_world");
    helpers::barca(&["assets", "refresh", &id.to_string()]).timeout(std::time::Duration::from_secs(30)).assert().success();

    // After reset, artifacts and DB should be gone
    helpers::reset();
    let fixture = helpers::fixture_dir();
    assert!(!fixture.join(".barca").exists(), ".barca/ should be removed after reset");
    assert!(!fixture.join(".barcafiles").exists(), ".barcafiles/ should be removed after reset");

    // assets list after reset+reindex should show "never run" again
    helpers::reindex();
    let output = helpers::assets_list();
    assert!(output.contains("never run"), "should show 'never run' after reset");
}
