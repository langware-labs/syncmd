//! Turn a path into concrete [`Asset`] groups with discovered git state.
//!
//! This is the only module that reads both the filesystem (to enumerate templated `{name}`
//! members) and git (via [`GitBackend`]). Its output feeds the pure [`crate::plan`].

use std::collections::BTreeSet;
use std::path::Path;

use crate::error::Result;
use crate::git::{GitBackend, RepoHandle};
use crate::id::{mint_id, mint_mount_id};
use crate::model::{
    Asset, AssetMount, AssetType, Baseline, MountState, Oid, Presence, Recency, Strategy, Transform,
};
use crate::registry::{Registry, Rule};

/// A discovered group: the logical asset, its per-mount state, the baseline, and the strategy.
#[derive(Debug, Clone)]
pub struct DiscoveredGroup {
    pub asset: Asset,
    pub states: Vec<MountState>,
    pub baseline: Baseline,
    pub strategy: Strategy,
}

/// Discover every in-scope group for `requested` (a file or dir inside `repo`).
pub fn discover(
    git: &dyn GitBackend,
    registry: &Registry,
    repo: &RepoHandle,
    requested: &Path,
) -> Result<Vec<DiscoveredGroup>> {
    let workdir = repo.workdir.clone();
    let files = list_repo_files(&workdir);
    let scope = Scope::new(&workdir, requested);

    let mut out = Vec::new();
    for rule in &registry.rules {
        for (asset_name, mounts) in expand_rule(rule, &files) {
            // Path scoping: keep the group iff any mount intersects the requested path.
            if !scope.matches(mounts.iter().map(|(_, p)| p.as_str())) {
                continue;
            }
            let group = build_group(git, repo, rule, &asset_name, &mounts)?;
            out.push(group);
        }
    }
    Ok(out)
}

/// A concrete (harness, repo-relative-path) member set for one group.
type Members = Vec<(String, String)>;

/// Expand a rule into concrete `(asset_name, members)` groups.
fn expand_rule(rule: &Rule, files: &BTreeSet<String>) -> Vec<(String, Members)> {
    if !rule.templated() {
        // Fixed file group: one group, the asset name is the rule group name.
        let members = rule
            .mounts
            .iter()
            .map(|m| (m.harness.clone(), m.pattern.clone()))
            .collect();
        return vec![(rule.group.clone(), members)];
    }

    // Templated: discover the set of `{name}`s present on disk across all mount patterns.
    let mut names: BTreeSet<String> = BTreeSet::new();
    for m in &rule.mounts {
        if !m.templated() {
            continue;
        }
        let (prefix, suffix) = split_template(&m.pattern);
        for f in files {
            if let Some(name) = extract_name(f, prefix, suffix) {
                names.insert(name);
            }
        }
    }

    names
        .into_iter()
        .map(|name| {
            let members = rule
                .mounts
                .iter()
                .map(|m| (m.harness.clone(), m.pattern.replace("{name}", &name)))
                .collect();
            (name, members)
        })
        .collect()
}

/// Split a templated pattern around the single `{name}` placeholder.
fn split_template(pattern: &str) -> (&str, &str) {
    let idx = pattern.find("{name}").expect("templated pattern");
    (&pattern[..idx], &pattern[idx + "{name}".len()..])
}

/// If `path` matches `prefix{name}suffix` with `{name}` spanning exactly one path component
/// (no `/`), return the name.
fn extract_name(path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let mid = rest.strip_suffix(suffix)?;
    if mid.is_empty() || mid.contains('/') {
        return None;
    }
    Some(mid.to_string())
}

/// Build a fully-populated [`DiscoveredGroup`] for one concrete member set.
fn build_group(
    git: &dyn GitBackend,
    repo: &RepoHandle,
    rule: &Rule,
    asset_name: &str,
    members: &Members,
) -> Result<DiscoveredGroup> {
    let paths: Vec<&str> = members.iter().map(|(_, p)| p.as_str()).collect();

    // Baseline: newest commit where >=2 members agreed.
    let (baseline, present_at_baseline) = compute_baseline(git, repo, &paths)?;

    // Per-mount state.
    let mut states = Vec::with_capacity(members.len());
    let mut mounts = Vec::with_capacity(members.len());
    for (harness, path) in members {
        let abs = repo.workdir.join(path);
        let present = abs.is_file();
        let cur_oid = if present {
            git.worktree_oid(repo, path)?
        } else {
            None
        };
        let recency = if !present {
            Recency::NegInf
        } else if git.is_dirty(repo, path)? {
            Recency::Now
        } else {
            match git.log_committer_epoch(repo, path)? {
                Some(t) => Recency::At(t),
                None => Recency::Now, // tracked-but-no-history edge → treat as live
            }
        };
        let at_baseline = present_at_baseline
            .get(path.as_str())
            .copied()
            .unwrap_or(false);

        let mount = AssetMount {
            type_: crate::model::MountType::AssetMount,
            id: Some(mint_mount_id(harness, path)),
            path: path.clone(),
            harness: harness.clone(),
            oid: cur_oid.clone(),
            transform: Transform::Identity,
        };
        mounts.push(mount.clone());
        states.push(MountState {
            mount,
            presence: if present {
                Presence::Present
            } else {
                Presence::Absent
            },
            cur_oid,
            present_at_baseline: at_baseline,
            recency,
        });
    }

    // Asset id: name-keyed types use the asset name; path-keyed use the canonical member path.
    let canonical_path = repo
        .workdir
        .join(&members[0].1)
        .to_string_lossy()
        .into_owned();
    let id = mint_id(rule.asset_type, asset_name, &canonical_path);
    let display_name = display_name(rule.asset_type, &rule.group, asset_name);

    let asset = Asset {
        type_: rule.asset_type,
        id,
        name: display_name,
        version: None,
        mounts,
    };

    Ok(DiscoveredGroup {
        asset,
        states,
        baseline,
        strategy: rule.strategy,
    })
}

fn display_name(ty: AssetType, group: &str, asset_name: &str) -> String {
    if group.contains("{name}") {
        asset_name.to_string()
    } else {
        let _ = ty;
        group.to_string()
    }
}

/// Walk the baseline candidate commits, returning the agreed OID (if any) and a map of which
/// member paths existed at that baseline commit.
fn compute_baseline(
    git: &dyn GitBackend,
    repo: &RepoHandle,
    paths: &[&str],
) -> Result<(Baseline, std::collections::HashMap<String, bool>)> {
    use std::collections::HashMap;
    let mut present_at_baseline: HashMap<String, bool> = HashMap::new();

    let commits = git.rev_list(repo, paths, 4096)?;
    for commit in &commits {
        let oids = git.ls_tree_oids_at(repo, commit, paths)?;
        let present: Vec<&Oid> = oids.iter().flatten().collect();
        if present.len() >= 2 {
            let first = present[0];
            if present.iter().all(|o| *o == first) {
                // Agreement. Record which paths existed here.
                for (p, o) in paths.iter().zip(oids.iter()) {
                    present_at_baseline.insert((*p).to_string(), o.is_some());
                }
                return Ok((Baseline::agreed(first.clone()), present_at_baseline));
            }
        }
    }
    // Bootstrap.
    Ok((Baseline::bootstrap(), present_at_baseline))
}

/// Path-scope filter: which groups are in scope for the requested path.
struct Scope {
    /// Repo-relative form of the requested path (`""` for the repo root).
    rel: String,
    is_dir: bool,
    is_root: bool,
}

impl Scope {
    fn new(workdir: &Path, requested: &Path) -> Scope {
        let canon = requested
            .canonicalize()
            .unwrap_or_else(|_| requested.to_path_buf());
        let wd = workdir.canonicalize().unwrap_or_else(|_| workdir.to_path_buf());
        let is_dir = canon.is_dir();
        let rel = canon
            .strip_prefix(&wd)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let is_root = rel.is_empty() || rel == ".";
        Scope { rel, is_dir, is_root }
    }

    fn matches<'a>(&self, mut member_paths: impl Iterator<Item = &'a str>) -> bool {
        if self.is_root {
            return true;
        }
        if self.is_dir {
            let dir_prefix = format!("{}/", self.rel);
            member_paths.any(|p| p == self.rel || p.starts_with(&dir_prefix))
        } else {
            // File: in scope iff it is one of the members.
            member_paths.any(|p| p == self.rel)
        }
    }
}

/// Enumerate repo-relative file paths under `workdir`, skipping `.git` and syncmd backups.
/// Dotfiles/dirs (e.g. `.claude`, `.github`) ARE traversed.
fn list_repo_files(workdir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let walker = ignore::WalkBuilder::new(workdir)
        .hidden(false) // include .claude, .github, etc.
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .map(|e| e == "bak" || e == "syncmd")
            .unwrap_or(false)
            || path.to_string_lossy().ends_with(".syncmd.bak")
        {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(workdir) {
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_name_single_component() {
        assert_eq!(
            extract_name(".claude/skills/foo/SKILL.md", ".claude/skills/", "/SKILL.md"),
            Some("foo".to_string())
        );
        // nested name (contains slash) rejected
        assert_eq!(
            extract_name(".claude/skills/a/b/SKILL.md", ".claude/skills/", "/SKILL.md"),
            None
        );
        // filename-stem template
        assert_eq!(
            extract_name(".claude/agents/rev.md", ".claude/agents/", ".md"),
            Some("rev".to_string())
        );
    }

    #[test]
    fn split_template_parts() {
        assert_eq!(
            split_template(".claude/skills/{name}/SKILL.md"),
            (".claude/skills/", "/SKILL.md")
        );
    }
}
