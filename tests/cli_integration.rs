#[allow(deprecated)]
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;

fn cmd() -> assert_cmd::Command {
    #[allow(deprecated)]
    let bin = cargo_bin("rust-cli-template");
    assert_cmd::Command::from(std::process::Command::new(bin))
}

#[test]
fn test_default_run_succeeds() {
    cmd()
        .assert()
        .success()
        .stdout(predicate::str::contains("IntLiteral"));
}

#[test]
fn test_json_format() {
    cmd()
        .args(&["--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\""));
}

#[test]
fn test_help_flag() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: rust-cli-template"))
        .stdout(predicate::str::contains("--format"));
}

#[test]
fn test_version_flag() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rust-cli-template 0.1.0"));
}

#[test]
fn test_invalid_input_file() {
    cmd()
        .args(&["--input", "nonexistent_file.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
