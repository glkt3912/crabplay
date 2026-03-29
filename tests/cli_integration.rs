use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crabplay"))
}

#[test]
fn test_list_succeeds() {
    // カレントディレクトリには MP3/FLAC がないため空リストで正常終了
    cmd()
        .args(["--list", "--dir", "."])
        .assert()
        .success();
}

#[test]
fn test_list_json_format() {
    cmd()
        .args(["--list", "--format", "json", "--dir", "."])
        .assert()
        .success();
}

#[test]
fn test_help_flag() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("crabplay"))
        .stdout(predicate::str::contains("--dir"))
        .stdout(predicate::str::contains("--list"));
}

#[test]
fn test_version_flag() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("crabplay 0.1.0"));
}

#[test]
fn test_nonexistent_dir_fails() {
    cmd()
        .args(["--list", "--dir", "/nonexistent/path/xyz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_invalid_format_fails() {
    cmd()
        .args(["--list", "--format", "xml", "--dir", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
