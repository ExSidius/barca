//! Workflow 3: Partitioned assets.
//! Tests that partition jobs run in parallel and produce correct artifacts.

mod helpers;

use serial_test::serial;

#[test]
#[serial]
fn test_small_partition_refresh() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let id = helpers::find_asset_id("fetch_prices");
    let output = helpers::barca(&["assets", "refresh", &id.to_string()])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    // fetch_prices has 3 partitions (AAPL, MSFT, GOOG)
    assert!(output.contains("3 job"), "should report 3 partition jobs");
    assert!(output.contains("success"), "should complete successfully");

    // Verify artifacts exist for each partition
    let fixture = helpers::fixture_dir();
    let barcafiles = fixture.join(".barcafiles");
    let fetch_prices_dir: Vec<_> = std::fs::read_dir(&barcafiles)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("fetch-prices"))
        .collect();
    assert!(!fetch_prices_dir.is_empty(), "should find fetch_prices artifact dir");

    // Count partition subdirectories
    let mut partition_count = 0;
    for entry in &fetch_prices_dir {
        for sub in walkdir(entry.path()) {
            if sub.file_name() == Some(std::ffi::OsStr::new("value.json")) {
                partition_count += 1;
            }
        }
    }
    assert_eq!(partition_count, 3, "should have 3 partition artifacts");
}

#[test]
#[serial]
fn test_partition_artifacts_contain_correct_keys() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let id = helpers::find_asset_id("fetch_prices");
    helpers::barca(&["assets", "refresh", &id.to_string()]).timeout(std::time::Duration::from_secs(30)).assert().success();

    // Collect all partition artifact values
    let fixture = helpers::fixture_dir();
    let barcafiles = fixture.join(".barcafiles");
    let mut tickers: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&barcafiles).unwrap().filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().contains("fetch-prices") {
            for value_path in walkdir(entry.path()) {
                if value_path.file_name() == Some(std::ffi::OsStr::new("value.json")) {
                    let content = std::fs::read_to_string(&value_path).unwrap();
                    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
                    if let Some(ticker) = value.get("ticker").and_then(|v| v.as_str()) {
                        tickers.push(ticker.to_string());
                    }
                }
            }
        }
    }

    tickers.sort();
    assert_eq!(tickers, vec!["AAPL", "GOOG", "MSFT"], "should have artifacts for all 3 tickers");
}

#[test]
#[serial]
fn test_large_partition_set_parallel_execution() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let id = helpers::find_asset_id("wide_asset");

    // Refresh 10000 partitions — the key test for parallelism
    let start = std::time::Instant::now();
    let output = helpers::barca(&["assets", "refresh", &id.to_string()])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let elapsed = start.elapsed();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("10000 job"), "should report 10000 partition jobs");
    assert!(output.contains("success"), "should complete successfully");

    // With 64 concurrent workers, 10000 trivial Python jobs should complete
    // well under 120s. If they were serial, each taking ~0.5s, it would take
    // ~5000s. With 64-way parallelism it should be ~80s worst case.
    eprintln!("[w3] 10000 partitions completed in {:.1}s (parallel execution verified)", elapsed.as_secs_f64());
    assert!(elapsed.as_secs() < 120, "10000 partitions took {}s — expected < 120s with parallel execution", elapsed.as_secs());
}

#[test]
#[serial]
fn test_partition_second_run_cached() {
    helpers::ensure_python_ready();
    helpers::reset();
    helpers::reindex();

    let id = helpers::find_asset_id("fetch_prices");
    // First run
    helpers::barca(&["assets", "refresh", &id.to_string()]).timeout(std::time::Duration::from_secs(30)).assert().success();

    // Second run should be instant (cached)
    let start = std::time::Instant::now();
    let output = helpers::barca(&["assets", "refresh", &id.to_string()])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let elapsed = start.elapsed();
    let output = String::from_utf8(output).unwrap();

    // Should either be "already fresh" or complete very quickly (cached)
    assert!(
        output.contains("already fresh") || elapsed.as_secs() < 5,
        "second partition refresh should be cached, took {:.1}s",
        elapsed.as_secs_f64()
    );
}

/// Recursively walk a directory and return all file paths.
fn walkdir(dir: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walkdir(path));
            } else {
                result.push(path);
            }
        }
    }
    result
}
