//! Shared integration-test harness: a temp git repo builder with deterministic commit dates.
//!
//! Uses the real `git` CLI to construct fixtures (so the gix backend is validated against
//! genuine repositories), with `GIT_COMMITTER_DATE`/`GIT_AUTHOR_DATE` pinned per commit so
//! recency-dependent tests never flake on wall-clock time.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

pub struct RepoFixture {
    pub dir: TempDir,
    commit_seq: i64,
}

impl RepoFixture {
    /// Create a temp dir and `git init` it with a fixed identity.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let f = RepoFixture { dir, commit_seq: 0 };
        f.git(&["init", "-q", "-b", "main"]);
        f.git(&["config", "user.name", "syncmd-test"]);
        f.git(&["config", "user.email", "test@syncmd.dev"]);
        f
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.dir.path())
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Write a file (creating parent dirs), without committing.
    pub fn write(&self, rel: &str, content: &str) -> &Self {
        let p = self.abs(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        self
    }

    /// Remove a file from the working tree.
    pub fn rm(&self, rel: &str) -> &Self {
        let p = self.abs(rel);
        if p.exists() {
            std::fs::remove_file(&p).unwrap();
        }
        self
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.abs(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.abs(rel).exists()
    }

    /// Stage everything and commit with a deterministic, monotonically increasing date.
    pub fn commit(&mut self, msg: &str) -> String {
        self.commit_seq += 1;
        let epoch = 1_767_225_600 + self.commit_seq * 86_400; // 2026-01-01 + N days
        let date = format!("@{epoch} +0000");
        self.git(&["add", "-A"]);
        let out = Command::new("git")
            .args(["commit", "-q", "-m", msg])
            .current_dir(self.dir.path())
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .expect("git commit");
        assert!(
            out.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }
}

/// Find a group report by display name.
pub fn group<'a>(
    report: &'a syncmd_core::SyncReport,
    name: &str,
) -> &'a syncmd_core::GroupReport {
    report
        .groups
        .iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| panic!("group {name:?} not found in report {:#?}", report.groups))
}

/// Whether a group has an action with the given verb for the given path.
pub fn has_action(g: &syncmd_core::GroupReport, path: &str, verb: &str) -> bool {
    g.mounts
        .iter()
        .any(|m| m.path == path && m.action == verb)
}
