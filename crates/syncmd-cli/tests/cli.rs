//! CLI-level tests: exit codes and JSON output through the real `syncmd` binary.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

/// Build a temp git repo with a fixed identity. Returns the dir.
fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    run_git(&dir, &["init", "-q", "-b", "main"]);
    run_git(&dir, &["config", "user.name", "t"]);
    run_git(&dir, &["config", "user.email", "t@t.dev"]);
    dir
}

fn run_git(dir: &TempDir, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir.path())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn write(dir: &TempDir, rel: &str, content: &str) {
    let p = dir.path().join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn commit(dir: &TempDir, msg: &str) {
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "-q", "-m", msg]);
}

#[test]
fn sync_in_sync_exits_0() {
    let dir = repo();
    write(&dir, "AGENTS.md", "x\n");
    write(&dir, "CLAUDE.md", "x\n");
    write(&dir, ".github/copilot-instructions.md", "x\n");
    write(&dir, ".cursorrules", "x\n");
    write(&dir, "GEMINI.md", "x\n");
    commit(&dir, "synced");

    Command::cargo_bin("syncmd")
        .unwrap()
        .args(["sync", "."])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("in sync"));
}

#[test]
fn sync_bootstrap_creates_and_exits_0() {
    let dir = repo();
    write(&dir, "CLAUDE.md", "seed\n");
    commit(&dir, "only claude");

    Command::cargo_bin("syncmd")
        .unwrap()
        .args(["sync", "."])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join("AGENTS.md").exists());
}

#[test]
fn conflict_strategy_error_exits_1() {
    let dir = repo();
    write(&dir, "AGENTS.md", "base\n");
    write(&dir, "CLAUDE.md", "base\n");
    commit(&dir, "base");
    write(&dir, "AGENTS.md", "a\n");
    commit(&dir, "edit a");
    write(&dir, "CLAUDE.md", "c\n");
    commit(&dir, "edit c");

    Command::cargo_bin("syncmd")
        .unwrap()
        .args(["sync", ".", "--strategy", "error"])
        .current_dir(dir.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("conflict"));
}

#[test]
fn not_a_repo_exits_2() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "x\n").unwrap();

    Command::cargo_bin("syncmd")
        .unwrap()
        .args(["sync", "."])
        .current_dir(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a git repository"));
}

#[test]
fn json_output_is_flow_sdk_shaped() {
    let dir = repo();
    write(&dir, "CLAUDE.md", "seed\n");
    commit(&dir, "only claude");

    let out = Command::cargo_bin("syncmd")
        .unwrap()
        .args(["plan", ".", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["type"], "sync_report");
    assert!(json["groups"].is_array());
    // plan writes nothing.
    assert!(!dir.path().join("AGENTS.md").exists());
}
