use assert_cmd::Command;
use predicates::prelude::*;

fn barca_cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("barca").unwrap()
}

#[test]
fn test_help_output() {
    barca_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Minimal asset orchestrator"))
        .stdout(predicate::str::contains("serve"))
        .stdout(predicate::str::contains("reindex"))
        .stdout(predicate::str::contains("reset"))
        .stdout(predicate::str::contains("assets"))
        .stdout(predicate::str::contains("jobs"));
}

#[test]
fn test_assets_help() {
    barca_cmd()
        .args(["assets", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("refresh"));
}

#[test]
fn test_jobs_help() {
    barca_cmd()
        .args(["jobs", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("show"));
}

#[test]
fn test_assets_list_no_config() {
    // All commands (except reset) now require barca.toml since they reindex first
    let tmp = tempfile::tempdir().unwrap();
    barca_cmd()
        .args(["assets", "list"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("barca.toml"));
}

#[test]
fn test_reindex_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    barca_cmd().arg("reindex").current_dir(tmp.path()).assert().failure().stderr(predicate::str::contains("barca.toml"));
}
