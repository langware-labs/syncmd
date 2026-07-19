//! `syncmd-core` — keep a group of equivalent AI-harness asset files converged on the latest
//! change.
//!
//! The crate is layered so the decision logic is a pure function:
//! * [`registry`] — declarative rules (built-in defaults + `syncmd.toml`).
//! * [`discovery`] — turn a path into [`model::Asset`] groups with discovered git state.
//! * [`git`] — the [`git::GitBackend`] query trait (gix impl behind the `gix-backend` feature).
//! * [`plan`] — the **pure** group decision ([`plan::plan_group`]).
//! * [`execute`] — apply the per-mount actions (atomic writes, backups).
//! * [`report`] — the flow-sdk-shaped [`report::SyncReport`].

pub mod discovery;
pub mod error;
pub mod execute;
pub mod git;
pub mod id;
pub mod model;
pub mod plan;
pub mod registry;
pub mod report;

use std::path::Path;

pub use error::{Error, Result};
pub use model::{
    Action, Asset, AssetMount, AssetType, Baseline, Decision, DecisionKind, MountState, Oid,
    PlanOpts, Strategy, WinnerReason,
};
pub use report::{GroupReport, GroupStatus, MountReport, SyncReport};

use crate::git::GitBackend;

/// Options for [`plan`] / [`sync`], mirroring the actionable CLI flags.
#[derive(Debug, Clone)]
pub struct SyncOpts {
    /// Restrict syncing to these harness labels (e.g. `claude`, `cursor`).
    /// `None` (the default) syncs every format in the registry.
    pub formats: Option<Vec<String>>,
    /// Override the per-rule strategy for every group.
    pub strategy: Option<Strategy>,
    /// Compute everything but write nothing (also the semantics of [`plan`]).
    pub dry_run: bool,
    /// Back up overwritten members to `<path>.syncmd.bak`.
    pub backup: bool,
    /// Create absent members from the winner.
    pub create_missing: bool,
    /// Propagate deletions across the group.
    pub allow_delete: bool,
    /// Reserved for multi-repo fan-out; single-repo discovery already recurses directories.
    pub recursive: bool,
}

impl Default for SyncOpts {
    fn default() -> Self {
        SyncOpts {
            formats: None,
            strategy: None,
            dry_run: false,
            backup: true,
            create_missing: true,
            allow_delete: false,
            recursive: false,
        }
    }
}

/// Discover + plan only. Writes nothing; the report's actions are marked `applied = false`.
pub fn plan(path: &Path, opts: &SyncOpts) -> Result<SyncReport> {
    let git = git::gix_backend::GixBackend::new();
    run(&git, path, opts, false)
}

/// Discover + plan + execute. Honors `opts.dry_run` (which suppresses writes).
pub fn sync(path: &Path, opts: &SyncOpts) -> Result<SyncReport> {
    let git = git::gix_backend::GixBackend::new();
    run(&git, path, opts, true)
}

/// The shared engine, generic over a [`GitBackend`] so tests can inject a fake.
///
/// `write` is `false` for [`plan`]; for [`sync`] writes happen unless `opts.dry_run`.
pub fn run(
    git: &dyn GitBackend,
    path: &Path,
    opts: &SyncOpts,
    write: bool,
) -> Result<SyncReport> {
    if !path.exists() {
        return Err(Error::BadPath(path.to_path_buf()));
    }
    let repo = git
        .discover_repo(path)?
        .ok_or_else(|| Error::NotARepo(path.to_path_buf()))?;

    let mut registry = registry::Registry::load(&repo.workdir)?;
    if let Some(formats) = &opts.formats {
        registry = registry.filtered(formats)?;
    }
    let groups = discovery::discover(git, &registry, &repo, path)?;

    let apply = write && !opts.dry_run;
    let mut report = SyncReport::new(repo.workdir.to_string_lossy().to_string());

    for g in &groups {
        let plan_opts = PlanOpts {
            strategy: opts.strategy.unwrap_or(g.strategy),
            backup: opts.backup,
            create_missing: opts.create_missing,
            allow_delete: opts.allow_delete,
        };
        let decision = plan::plan_group(&g.asset, &g.states, &g.baseline, plan_opts);
        let diverged = plan::was_divergence(&g.states, &g.baseline);
        let overridden = plan::overridden_losers(&decision, &g.states, &g.baseline);

        if apply {
            execute::execute(&repo, &decision, opts.dry_run)?;
        }

        let note = group_note(&decision, &repo);
        let gr = report::group_report(
            &g.asset,
            &decision,
            &g.baseline,
            diverged,
            overridden,
            note,
            apply,
        );
        report.groups.push(gr);
    }

    report.finalize();
    Ok(report)
}

fn group_note(decision: &Decision, repo: &git::RepoHandle) -> Option<String> {
    let mut notes = Vec::new();
    if let DecisionKind::Skipped { reason } = &decision.kind {
        if reason == "delete_blocked" {
            notes.push("deletion not propagated; pass --allow-delete to apply".to_string());
        }
    }
    if repo.shallow {
        notes.push("shallow clone: baseline may be beyond the history boundary".to_string());
    }
    if notes.is_empty() {
        None
    } else {
        Some(notes.join("; "))
    }
}
