//! Integration tests over real temp git repos, exercising the gix backend + discovery +
//! baseline walk + execute through the public `plan`/`sync` API.

mod common;

use common::{group, has_action, RepoFixture};
use syncmd_core::report::GroupStatus;
use syncmd_core::{plan, sync, SyncOpts};

fn opts() -> SyncOpts {
    SyncOpts::default()
}

// ---- Instructions (fixed-name file group) ----

#[test]
fn in_sync_is_noop() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "hello\n")
        .write("CLAUDE.md", "hello\n")
        .write(".github/copilot-instructions.md", "hello\n")
        .write(".cursorrules", "hello\n")
        .write("GEMINI.md", "hello\n");
    f.commit("init synced");

    // Restrict to the five formats written above; the full registry would
    // create the remaining mounts and report Propagated instead.
    let o = SyncOpts {
        formats: Some(
            ["agents", "claude", "copilot", "cursor", "gemini"]
                .map(String::from)
                .to_vec(),
        ),
        ..opts()
    };
    let report = sync(f.path(), &o).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::InSync);
    assert_eq!(report.exit_code(), 0);
}

#[test]
fn formats_filter_limits_mounts_and_rejects_unknown() {
    let mut f = RepoFixture::new();
    f.write("CLAUDE.md", "seed\n");
    f.commit("only claude");

    // Only claude+agents: exactly one file is created, none of the other 17.
    let o = SyncOpts {
        formats: Some(vec!["claude".into(), "agents".into()]),
        ..opts()
    };
    let report = sync(f.path(), &o).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Propagated);
    assert!(has_action(g, "AGENTS.md", "create"));
    assert!(f.path().join("AGENTS.md").exists());
    assert!(!f.path().join("GEMINI.md").exists());
    assert!(!f.path().join(".cursorrules").exists());

    // Unknown labels are a config error.
    let bad = SyncOpts {
        formats: Some(vec!["claude".into(), "notreal".into()]),
        ..opts()
    };
    assert!(sync(f.path(), &bad).is_err());
}

#[test]
fn bootstrap_creates_missing_members() {
    let mut f = RepoFixture::new();
    f.write("CLAUDE.md", "seed content\n");
    f.commit("only claude");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Propagated);
    assert_eq!(g.winner_path.as_deref(), Some("CLAUDE.md"));
    // The other members are created with identical content.
    assert!(f.exists("AGENTS.md"));
    assert_eq!(f.read("AGENTS.md"), "seed content\n");
    assert!(f.exists("GEMINI.md"));
    assert!(has_action(g, "AGENTS.md", "create"));
}

#[test]
fn single_committed_update_propagates() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "v1\n").write("CLAUDE.md", "v1\n");
    f.commit("synced v1");
    // Edit CLAUDE.md and commit it (so it is the lone change since baseline).
    f.write("CLAUDE.md", "v2\n");
    f.commit("edit claude");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Propagated);
    assert_eq!(g.winner_path.as_deref(), Some("CLAUDE.md"));
    assert_eq!(f.read("AGENTS.md"), "v2\n");
    // Backup of the overwritten member exists.
    assert!(f.exists("AGENTS.md.syncmd.bak"));
    assert_eq!(f.read("AGENTS.md.syncmd.bak"), "v1\n");
}

#[test]
fn dirty_working_edit_beats_old_commit() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "v1\n").write("CLAUDE.md", "v1\n");
    f.commit("synced v1");
    // Uncommitted edit to CLAUDE.md → ranks as NOW, wins over committed AGENTS.md.
    f.write("CLAUDE.md", "live\n");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.winner_path.as_deref(), Some("CLAUDE.md"));
    assert_eq!(f.read("AGENTS.md"), "live\n");
}

#[test]
fn divergence_newest_wins_and_backs_up_loser() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "base\n").write("CLAUDE.md", "base\n");
    f.commit("synced base");
    // Older edit to AGENTS.md, newer edit to CLAUDE.md → both changed since baseline.
    f.write("AGENTS.md", "agents-edit\n");
    f.commit("edit agents");
    f.write("CLAUDE.md", "claude-edit\n");
    f.commit("edit claude");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::DivergedResolved);
    assert_eq!(g.winner_path.as_deref(), Some("CLAUDE.md"));
    assert!(g.overridden.contains(&"AGENTS.md".to_string()));
    assert_eq!(f.read("AGENTS.md"), "claude-edit\n");
    assert!(f.exists("AGENTS.md.syncmd.bak"));
}

#[test]
fn divergence_error_strategy_conflicts_and_writes_nothing() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "base\n").write("CLAUDE.md", "base\n");
    f.commit("synced base");
    f.write("AGENTS.md", "a\n");
    f.commit("edit agents");
    f.write("CLAUDE.md", "c\n");
    f.commit("edit claude");

    let o = SyncOpts {
        strategy: Some(syncmd_core::Strategy::Error),
        ..opts()
    };
    let report = sync(f.path(), &o).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Conflict);
    assert_eq!(report.exit_code(), 1);
    // Nothing rewritten.
    assert_eq!(f.read("AGENTS.md"), "a\n");
    assert!(!f.exists("AGENTS.md.syncmd.bak"));
}

#[test]
fn idempotent_rerun_is_noop_with_no_new_backups() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "v1\n").write("CLAUDE.md", "v1\n");
    f.commit("synced v1");
    f.write("CLAUDE.md", "v2\n");
    f.commit("edit");

    let r1 = sync(f.path(), &opts()).unwrap();
    assert_eq!(group(&r1, "instructions").status, GroupStatus::Propagated);
    // Remove the backup the first run made, then re-run.
    f.rm("AGENTS.md.syncmd.bak");
    let r2 = sync(f.path(), &opts()).unwrap();
    assert_eq!(group(&r2, "instructions").status, GroupStatus::InSync);
    assert!(!f.exists("AGENTS.md.syncmd.bak"), "no new backup on no-op");
}

#[test]
fn dry_run_writes_nothing() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "v1\n").write("CLAUDE.md", "v1\n");
    f.commit("synced v1");
    f.write("CLAUDE.md", "v2\n");
    f.commit("edit");

    let o = SyncOpts {
        dry_run: true,
        ..opts()
    };
    let report = sync(f.path(), &o).unwrap();
    let g = group(&report, "instructions");
    // Plan is computed...
    assert_eq!(g.winner_path.as_deref(), Some("CLAUDE.md"));
    // ...but disk is untouched.
    assert_eq!(f.read("AGENTS.md"), "v1\n");
    assert!(!f.exists("AGENTS.md.syncmd.bak"));
    assert!(g.mounts.iter().all(|m| !m.applied));
}

#[test]
fn plan_never_writes() {
    let mut f = RepoFixture::new();
    f.write("CLAUDE.md", "seed\n");
    f.commit("only claude");

    let report = plan(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert!(has_action(g, "AGENTS.md", "create"));
    assert!(!f.exists("AGENTS.md"), "plan must not create files");
}

// ---- Deletion semantics ----

#[test]
fn deletion_blocked_by_default() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "x\n").write("CLAUDE.md", "x\n");
    f.commit("synced");
    f.rm("CLAUDE.md"); // delete one member (uncommitted)

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Skipped);
    assert!(f.exists("AGENTS.md"), "peer must not be deleted");
}

#[test]
fn deletion_allowed_propagates() {
    let mut f = RepoFixture::new();
    f.write("AGENTS.md", "x\n").write("CLAUDE.md", "x\n");
    f.commit("synced");
    f.rm("CLAUDE.md");

    let o = SyncOpts {
        allow_delete: true,
        ..opts()
    };
    let report = sync(f.path(), &o).unwrap();
    let g = group(&report, "instructions");
    assert_eq!(g.status, GroupStatus::Propagated);
    assert!(!f.exists("AGENTS.md"), "peer should be deleted");
    assert!(f.exists("AGENTS.md.syncmd.bak"));
}

// ---- Folder-backed skill group ----

#[test]
fn skill_folder_bootstrap_across_roots() {
    let mut f = RepoFixture::new();
    f.write(".claude/skills/foo/SKILL.md", "skill body\n");
    f.commit("only claude skill");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "foo");
    assert_eq!(g.status, GroupStatus::Propagated);
    assert!(f.exists(".agents/skills/foo/SKILL.md"));
    assert!(f.exists(".github/skills/foo/SKILL.md"));
    assert_eq!(f.read(".agents/skills/foo/SKILL.md"), "skill body\n");
}

#[test]
fn spec_folder_single_update() {
    let mut f = RepoFixture::new();
    f.write(".claude/specs/api/spec.md", "spec v1\n")
        .write(".agents/specs/api/spec.md", "spec v1\n");
    f.commit("synced spec");
    f.write(".claude/specs/api/spec.md", "spec v2\n");

    let report = sync(f.path(), &opts()).unwrap();
    let g = group(&report, "api");
    assert_eq!(g.winner_path.as_deref(), Some(".claude/specs/api/spec.md"));
    assert_eq!(f.read(".agents/specs/api/spec.md"), "spec v2\n");
}

// ---- Path scope ----

#[test]
fn file_scope_only_syncs_that_group() {
    let mut f = RepoFixture::new();
    f.write("CLAUDE.md", "seed\n");
    f.write(".claude/skills/foo/SKILL.md", "skill\n");
    f.commit("init");

    // Target a single file → only the instructions group is in scope.
    let report = sync(&f.abs("CLAUDE.md"), &opts()).unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].name, "instructions");
}

#[test]
fn skill_name_dir_expands_to_all_roots() {
    let mut f = RepoFixture::new();
    f.write(".claude/skills/foo/SKILL.md", "skill\n");
    f.commit("init");

    // Target only the .claude copy's directory; group must still cover all roots (P6).
    let report = sync(&f.abs(".claude/skills/foo"), &opts()).unwrap();
    let g = group(&report, "foo");
    assert!(f.exists(".agents/skills/foo/SKILL.md"));
    assert!(f.exists(".github/skills/foo/SKILL.md"));
    let _ = g;
}

// ---- Preconditions ----

#[test]
fn not_a_repo_errors_exit_2() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "x\n").unwrap();
    let err = sync(dir.path(), &opts()).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn bad_path_errors_exit_2() {
    let f = RepoFixture::new();
    let missing = f.abs("does/not/exist.md");
    let err = sync(&missing, &opts()).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}
