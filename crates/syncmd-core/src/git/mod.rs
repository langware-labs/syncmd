//! The git query surface. Keeping it a trait is what lets [`crate::plan::plan_group`] stay a
//! pure, I/O-free function: discovery fills [`crate::model::MountState`] from a `GitBackend`,
//! then the decision is computed over plain data. Tests use a fake backend; production uses the
//! `gix` backend in [`gix_backend`].

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::model::Oid;

#[cfg(feature = "gix-backend")]
pub mod gix_backend;

/// An opened repository: its work-tree root and the git dir.
#[derive(Debug, Clone)]
pub struct RepoHandle {
    pub workdir: PathBuf,
    pub git_dir: PathBuf,
    /// True if the history is shallow (baseline detection may hit a boundary).
    pub shallow: bool,
}

/// The minimal, mostly read-only set of git queries syncmd needs.
pub trait GitBackend {
    /// Discover the repo containing `path`; `Ok(None)` if `path` is not in a work tree.
    fn discover_repo(&self, path: &Path) -> Result<Option<RepoHandle>>;

    /// HEAD-tree blob OIDs for repo-relative `paths` (batched). Missing path → `None`.
    fn ls_tree_oids(&self, repo: &RepoHandle, paths: &[&str]) -> Result<Vec<Option<Oid>>>;

    /// Blob OID of the working-tree file at `path` (hash-object), `None` if absent.
    fn worktree_oid(&self, repo: &RepoHandle, path: &str) -> Result<Option<Oid>>;

    /// True if the working-tree file differs from HEAD, or is untracked (→ recency `Now`).
    fn is_dirty(&self, repo: &RepoHandle, path: &str) -> Result<bool>;

    /// Committer epoch (unix secs) of the last commit touching `path`; `None` if untracked.
    fn log_committer_epoch(&self, repo: &RepoHandle, path: &str) -> Result<Option<i64>>;

    /// Blob OIDs of `paths` at a specific commit (for the baseline walk). Missing → `None`.
    fn ls_tree_oids_at(
        &self,
        repo: &RepoHandle,
        commit: &Oid,
        paths: &[&str],
    ) -> Result<Vec<Option<Oid>>>;

    /// Commit OIDs (newest-first) that touched any of `paths`, capped at `max`.
    fn rev_list(&self, repo: &RepoHandle, paths: &[&str], max: usize) -> Result<Vec<Oid>>;

    /// Raw bytes of a blob OID — called once per group (the winner) at write time.
    fn cat_file_blob(&self, repo: &RepoHandle, oid: &Oid) -> Result<Vec<u8>>;
}
