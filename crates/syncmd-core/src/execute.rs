//! Apply a group's per-mount [`Action`]s to the working tree.
//!
//! The only mutating module. Writes are atomic (temp file in the same dir + rename) so a crash
//! never leaves a half-written asset; overwritten files are backed up to `<path>.syncmd.bak`
//! first. Under `dry_run` nothing touches disk.

use std::io::Write as _;
use std::path::Path;

use crate::error::Result;
use crate::git::RepoHandle;
use crate::model::{Action, Decision, DecisionKind};

/// The backup suffix appended to overwritten/deleted members.
pub const BACKUP_SUFFIX: &str = ".syncmd.bak";

/// Execute `decision`'s actions. Under `dry_run` nothing touches disk.
///
/// The winner's bytes are read once from the **winner's working-tree file** — this is correct
/// whether the winner is committed or a live (dirty) edit, and avoids depending on the blob
/// being present in the object database.
pub fn execute(repo: &RepoHandle, decision: &Decision, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    // Read the winner content once, if any write/create needs it.
    let needs_content = decision
        .actions
        .iter()
        .any(|a| matches!(a, Action::Write { .. } | Action::Create { .. }));
    let content: Option<Vec<u8>> = if needs_content {
        let winner_path = match &decision.kind {
            DecisionKind::Propagate { winner_path, .. }
            | DecisionKind::Bootstrap { winner_path } => winner_path.clone(),
            _ => return Ok(()), // no winner content to propagate
        };
        Some(std::fs::read(repo.workdir.join(&winner_path))?)
    } else {
        None
    };

    for action in &decision.actions {
        match action {
            Action::Skip { .. } => {}
            Action::Backup { path } => {
                backup(&repo.workdir.join(path))?;
            }
            Action::Write { path, .. } | Action::Create { path, .. } => {
                let bytes = content
                    .as_ref()
                    .expect("winner content fetched for write/create");
                atomic_write(&repo.workdir.join(path), bytes)?;
            }
            Action::Delete { path } => {
                let abs = repo.workdir.join(path);
                if abs.exists() {
                    std::fs::remove_file(&abs)?;
                }
            }
        }
    }
    Ok(())
}

/// Copy an existing file to `<path>.syncmd.bak` before it is overwritten/removed. No-op if the
/// file does not exist.
fn backup(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut bak = path.as_os_str().to_owned();
    bak.push(BACKUP_SUFFIX);
    std::fs::copy(path, Path::new(&bak))?;
    Ok(())
}

/// Write `bytes` to `path` atomically: temp file in the same directory, then rename. Parent
/// directories are created as needed.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
