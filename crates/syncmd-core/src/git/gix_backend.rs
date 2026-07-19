//! `gix`-backed [`GitBackend`]. The only module that imports `gix`; everything else works
//! through the trait so the decision logic stays pure and testable.

use std::path::Path;

use gix::ObjectId;

use crate::error::{Error, Result};
use crate::git::{GitBackend, RepoHandle};
use crate::model::Oid;

/// A [`GitBackend`] implemented with gitoxide.
#[derive(Debug, Default, Clone, Copy)]
pub struct GixBackend;

impl GixBackend {
    pub fn new() -> Self {
        GixBackend
    }

    fn open(&self, repo: &RepoHandle) -> Result<gix::Repository> {
        gix::open(&repo.git_dir).map_err(|e| Error::Git(e.to_string()))
    }
}

fn to_oid(id: ObjectId) -> Oid {
    Oid::new(id.to_hex().to_string())
}

fn parse_oid(oid: &Oid) -> Result<ObjectId> {
    ObjectId::from_hex(oid.as_str().as_bytes()).map_err(|e| Error::Git(e.to_string()))
}

/// Blob OID of `path` within `commit_id`'s tree, or `None` if the path is absent there.
fn path_oid_in_commit(
    repo: &gix::Repository,
    commit_id: ObjectId,
    path: &str,
) -> Result<Option<ObjectId>> {
    let commit = repo
        .find_object(commit_id)
        .map_err(|e| Error::Git(e.to_string()))?
        .try_into_commit()
        .map_err(|e| Error::Git(e.to_string()))?;
    let tree = commit.tree().map_err(|e| Error::Git(e.to_string()))?;
    let mut buf = Vec::new();
    match tree
        .lookup_entry_by_path(Path::new(path), &mut buf)
        .map_err(|e| Error::Git(e.to_string()))?
    {
        Some(entry) => Ok(Some(entry.oid().to_owned())),
        None => Ok(None),
    }
}

/// All commit ids reachable from HEAD, newest-first. Empty if there are no commits.
fn ancestors(repo: &gix::Repository) -> Result<Vec<ObjectId>> {
    let head = match repo.head_commit() {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    let walk = repo
        .rev_walk(Some(head.id))
        .all()
        .map_err(|e| Error::Git(e.to_string()))?;
    for info in walk {
        let info = info.map_err(|e| Error::Git(e.to_string()))?;
        out.push(info.id);
    }
    Ok(out)
}

/// Did `commit` change `path` relative to its parents? True if the path's blob differs from
/// every parent (or the commit is a root with the path present).
fn commit_touched(
    repo: &gix::Repository,
    commit_id: ObjectId,
    path: &str,
) -> Result<bool> {
    let here = path_oid_in_commit(repo, commit_id, path)?;
    let commit = repo
        .find_object(commit_id)
        .map_err(|e| Error::Git(e.to_string()))?
        .try_into_commit()
        .map_err(|e| Error::Git(e.to_string()))?;
    let parents: Vec<ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
    if parents.is_empty() {
        return Ok(here.is_some());
    }
    for p in parents {
        let there = path_oid_in_commit(repo, p, path)?;
        if there == here {
            return Ok(false); // unchanged vs at least one parent
        }
    }
    // Differs from every parent → this commit added, modified, or deleted the path.
    Ok(true)
}

impl GitBackend for GixBackend {
    fn discover_repo(&self, path: &Path) -> Result<Option<RepoHandle>> {
        // gix::discover wants a directory; if given a file, start from its parent.
        let start = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let repo = match gix::discover(start) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        let workdir = match repo.work_dir() {
            Some(w) => w.to_path_buf(),
            None => return Ok(None), // bare repo — nothing to sync
        };
        let git_dir = repo.git_dir().to_path_buf();
        let shallow = repo.is_shallow();
        Ok(Some(RepoHandle {
            workdir,
            git_dir,
            shallow,
        }))
    }

    fn ls_tree_oids(&self, repo: &RepoHandle, paths: &[&str]) -> Result<Vec<Option<Oid>>> {
        let grepo = self.open(repo)?;
        let head = match grepo.head_commit() {
            Ok(c) => c.id,
            Err(_) => return Ok(vec![None; paths.len()]),
        };
        let mut out = Vec::with_capacity(paths.len());
        for p in paths {
            out.push(path_oid_in_commit(&grepo, head, p)?.map(to_oid));
        }
        Ok(out)
    }

    fn worktree_oid(&self, repo: &RepoHandle, path: &str) -> Result<Option<Oid>> {
        let abs = repo.workdir.join(path);
        if !abs.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&abs)?;
        let grepo = self.open(repo)?;
        // Compute the blob id WITHOUT writing it into the object database.
        let id = gix::objs::compute_hash(grepo.object_hash(), gix::objs::Kind::Blob, &bytes);
        Ok(Some(to_oid(id)))
    }

    fn is_dirty(&self, repo: &RepoHandle, path: &str) -> Result<bool> {
        let head = self.ls_tree_oids(repo, &[path])?.into_iter().next().flatten();
        let work = self.worktree_oid(repo, path)?;
        match (head, work) {
            (Some(h), Some(w)) => Ok(h != w),
            (None, Some(_)) => Ok(true), // untracked
            (Some(_), None) => Ok(true), // deleted in worktree
            (None, None) => Ok(false),
        }
    }

    fn log_committer_epoch(&self, repo: &RepoHandle, path: &str) -> Result<Option<i64>> {
        let grepo = self.open(repo)?;
        for commit_id in ancestors(&grepo)? {
            if commit_touched(&grepo, commit_id, path)? {
                let commit = grepo
                    .find_object(commit_id)
                    .map_err(|e| Error::Git(e.to_string()))?
                    .try_into_commit()
                    .map_err(|e| Error::Git(e.to_string()))?;
                let sig = commit.committer().map_err(|e| Error::Git(e.to_string()))?;
                return Ok(Some(sig.time.seconds));
            }
        }
        Ok(None)
    }

    fn ls_tree_oids_at(
        &self,
        repo: &RepoHandle,
        commit: &Oid,
        paths: &[&str],
    ) -> Result<Vec<Option<Oid>>> {
        let grepo = self.open(repo)?;
        let cid = parse_oid(commit)?;
        let mut out = Vec::with_capacity(paths.len());
        for p in paths {
            out.push(path_oid_in_commit(&grepo, cid, p)?.map(to_oid));
        }
        Ok(out)
    }

    fn rev_list(&self, repo: &RepoHandle, paths: &[&str], max: usize) -> Result<Vec<Oid>> {
        let grepo = self.open(repo)?;
        let mut out = Vec::new();
        for commit_id in ancestors(&grepo)? {
            if out.len() >= max {
                break;
            }
            let mut touched = false;
            for p in paths {
                if commit_touched(&grepo, commit_id, p)? {
                    touched = true;
                    break;
                }
            }
            if touched {
                out.push(to_oid(commit_id));
            }
        }
        Ok(out)
    }

    fn cat_file_blob(&self, repo: &RepoHandle, oid: &Oid) -> Result<Vec<u8>> {
        let grepo = self.open(repo)?;
        let id = parse_oid(oid)?;
        let obj = grepo
            .find_object(id)
            .map_err(|e| Error::Git(e.to_string()))?;
        Ok(obj.data.clone())
    }
}
