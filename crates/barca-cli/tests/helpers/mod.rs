use std::path::PathBuf;
use std::process::Command;

/// Path to the basic_app example project (the default integration test fixture).
pub fn fixture_dir() -> PathBuf {
    fixture_dir_for("basic_app")
}

/// Path to a named example project under `examples/`.
pub fn fixture_dir_for(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join(format!("../../examples/{name}"))
        .canonicalize()
        .unwrap_or_else(|_| panic!("example '{}' not found", name))
}

/// Build and return a Command for the `barca` CLI binary, with cwd set to
/// the fixture project and RUST_LOG suppressed so test output stays clean.
pub fn barca(args: &[&str]) -> assert_cmd::Command {
    barca_in(fixture_dir(), args)
}

/// Like `barca()` but with an explicit working directory.
pub fn barca_in(dir: PathBuf, args: &[&str]) -> assert_cmd::Command {
    #[allow(deprecated)]
    let mut cmd = assert_cmd::Command::cargo_bin("barca").unwrap();
    cmd.current_dir(dir);
    cmd.env("RUST_LOG", "error");
    cmd.args(args);
    cmd
}

/// Run `barca reset --db --artifacts --tmp` to get a clean slate.
pub fn reset() {
    barca(&["reset", "--db", "--artifacts", "--tmp"]).assert().success();
}

/// Run `barca reset` in a specific fixture directory.
pub fn reset_in(dir: &PathBuf) {
    barca_in(dir.clone(), &["reset", "--db", "--artifacts", "--tmp"]).assert().success();
}

/// Run `barca reindex` and return stdout.
pub fn reindex() -> String {
    let output = barca(&["reindex"]).assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Run `barca reindex` in a specific fixture directory and return stdout.
pub fn reindex_in(dir: &PathBuf) -> String {
    let output = barca_in(dir.clone(), &["reindex"]).assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Run `barca assets list` and return stdout.
pub fn assets_list() -> String {
    let output = barca(&["assets", "list"]).assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Run `barca assets list` in a specific fixture directory and return stdout.
pub fn assets_list_in(dir: &PathBuf) -> String {
    let output = barca_in(dir.clone(), &["assets", "list"]).assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Parse `assets list` output to find the ID for an asset whose Name column
/// contains `name_substring`. Panics if not found.
pub fn find_asset_id(name_substring: &str) -> i64 {
    find_asset_id_in(&fixture_dir(), name_substring)
}

/// Parse `assets list` output from a specific directory to find the ID.
pub fn find_asset_id_in(dir: &PathBuf, name_substring: &str) -> i64 {
    let output = assets_list_in(dir);
    for line in output.lines() {
        if line.contains(name_substring) {
            // Table format: "│ ID ┆ Name ┆ ..."
            // Extract the first column (ID) after the leading "│"
            let trimmed = line.trim_start_matches('│').trim();
            if let Some(id_str) = trimmed.split('┆').next() {
                if let Ok(id) = id_str.trim().parse::<i64>() {
                    return id;
                }
            }
        }
    }
    panic!("asset containing '{}' not found in:\n{}", name_substring, output);
}

/// Check that the Python environment is ready (barca package importable).
/// Panics with a helpful message if not.
pub fn ensure_python_ready() {
    ensure_python_ready_in(&fixture_dir());
}

/// Check that the Python environment is ready in a specific fixture directory.
pub fn ensure_python_ready_in(dir: &PathBuf) {
    let result = Command::new("uv").args(["run", "python", "-c", "import barca"]).current_dir(dir).output();
    match result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            panic!(
                "Python environment not ready — `import barca` failed.\n\
                 Run `just build-py && cd {} && uv sync` first.\n\
                 stderr: {}",
                dir.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            panic!("Failed to run `uv`: {}. Is uv installed?", e);
        }
    }
}
