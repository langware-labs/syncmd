//! Core entity model.
//!
//! Entity model (Option A): an [`Asset`] is the *logical* group that owns the canonical
//! content (via the winning blob OID) and a stable id; it **has-many** [`AssetMount`], each a
//! physical file at a path for one harness. A future "Manifest" is just an [`AssetMount`] with
//! a non-identity [`Transform`] — never a separate entity.
//!
//! Decisions ([`crate::plan`]) are made at the **group** level (one winner); actions are
//! applied **per mount**.
//!
//! Serde policy (matches flowpad-sdk record JSON): the `type` field is emitted first, then
//! `id` (omitted when `None`), then the rest; field names are snake_case; `None` fields are
//! omitted. Struct field order is load-bearing — it guarantees `type`-first output.

use serde::{Deserialize, Serialize};

/// A blob object id, rendered as a hex string. Newtype so it can never be confused with a
/// path or arbitrary string in the decision logic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Oid(pub String);

impl Oid {
    pub fn new(hex: impl Into<String>) -> Self {
        Oid(hex.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// flowpad-sdk asset `type` strings. Serializes as the bare snake_case value
/// (`skill`, `agent`, `markdown`, `spec`, `claude_md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Skill,
    Agent,
    Markdown,
    Spec,
    ClaudeMd,
}

/// How an asset's stable id is keyed — mirrors flow-sdk's per-type minting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKey {
    /// `uuid5(NAMESPACE_DNS, "<type>:<name>")` — skill / agent.
    Name,
    /// `uuid5(NAMESPACE_URL, <resolved_path>)` — markdown / spec / claude_md.
    ResolvedPath,
}

impl AssetType {
    /// The bare snake_case string for this type (matches the serde representation).
    pub fn as_str(self) -> &'static str {
        match self {
            AssetType::Skill => "skill",
            AssetType::Agent => "agent",
            AssetType::Markdown => "markdown",
            AssetType::Spec => "spec",
            AssetType::ClaudeMd => "claude_md",
        }
    }

    /// Which key source this type's id is derived from.
    pub fn id_key(self) -> IdKey {
        match self {
            AssetType::Skill | AssetType::Agent => IdKey::Name,
            AssetType::Markdown | AssetType::Spec | AssetType::ClaudeMd => IdKey::ResolvedPath,
        }
    }

    /// Whether this asset is folder-backed (the OID-bearing main file lives inside a folder).
    pub fn is_folder_backed(self) -> bool {
        matches!(self, AssetType::Skill | AssetType::Spec)
    }
}

/// The `type` discriminator for a mount sub-record (every flow-sdk record carries a `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountType {
    AssetMount,
}

/// How canonical content is written into a mount. v1 only mirrors verbatim; a non-identity
/// transform is the future "Manifest" (adapter compiler) seam.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    #[default]
    Identity,
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        matches!(self, Transform::Identity)
    }
}

/// A physical file at a path for one harness — the spec's "member".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetMount {
    #[serde(rename = "type")]
    pub type_: MountType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Repo-relative path of the file.
    pub path: String,
    /// Harness label, e.g. `claude`, `agents`, `copilot`.
    pub harness: String,
    /// The current blob OID (working tree if present, else HEAD); `None` if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid: Option<Oid>,
    #[serde(default, skip_serializing_if = "Transform::is_identity")]
    pub transform: Transform,
}

impl AssetMount {
    pub fn new(path: impl Into<String>, harness: impl Into<String>) -> Self {
        AssetMount {
            type_: MountType::AssetMount,
            id: None,
            path: path.into(),
            harness: harness.into(),
            oid: None,
            transform: Transform::Identity,
        }
    }
}

/// A logical asset: a group of equivalent mounts that should all carry the same content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    #[serde(rename = "type")]
    pub type_: AssetType,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub mounts: Vec<AssetMount>,
}

/// Whether a mount's file is present in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent,
}

/// Comparable recency of a mount. Ordering: `Now` > `At(epoch)` > `NegInf`.
///
/// Dirty/untracked working-tree edits are `Now` so a live edit beats older commits (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recency {
    NegInf,
    At(i64),
    Now,
}

impl Recency {
    fn rank(self) -> (u8, i64) {
        match self {
            Recency::NegInf => (0, i64::MIN),
            Recency::At(t) => (1, t),
            Recency::Now => (2, i64::MAX),
        }
    }
}

impl PartialOrd for Recency {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Recency {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// Discovered per-mount git state — the only input [`crate::plan::plan_group`] reads.
#[derive(Debug, Clone)]
pub struct MountState {
    pub mount: AssetMount,
    pub presence: Presence,
    /// Current content OID (working tree if present, else HEAD), `None` if absent.
    pub cur_oid: Option<Oid>,
    /// Whether this mount's path existed at the baseline commit (drives create vs delete).
    pub present_at_baseline: bool,
    pub recency: Recency,
}

impl MountState {
    pub fn path(&self) -> &str {
        &self.mount.path
    }
}

/// The baseline: the agreed content OID from the most recent commit where the group concurred,
/// or `None` for bootstrap (the group has never been in sync).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baseline {
    pub oid: Option<Oid>,
}

impl Baseline {
    pub fn agreed(oid: Oid) -> Self {
        Baseline { oid: Some(oid) }
    }
    pub fn bootstrap() -> Self {
        Baseline { oid: None }
    }
    pub fn is_bootstrap(&self) -> bool {
        self.oid.is_none()
    }
}

/// Divergence-resolution strategy (`--strategy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    #[default]
    Newest,
    Error,
    Interactive,
}

/// Options that shape the decision (mirrors the actionable CLI flags).
#[derive(Debug, Clone, Copy)]
pub struct PlanOpts {
    pub strategy: Strategy,
    pub backup: bool,
    pub create_missing: bool,
    pub allow_delete: bool,
}

impl Default for PlanOpts {
    fn default() -> Self {
        PlanOpts {
            strategy: Strategy::Newest,
            backup: true,
            create_missing: true,
            allow_delete: false,
        }
    }
}

/// Why a particular mount became the winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinnerReason {
    SingleChange,
    Newest,
    Chosen,
    Bootstrap,
}

/// Group-level outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionKind {
    /// Every member present and equal — nothing to do.
    InSync,
    /// A single winner propagates to the rest.
    Propagate {
        winner_path: String,
        reason: WinnerReason,
    },
    /// First-time fill: no baseline ever existed.
    Bootstrap { winner_path: String },
    /// Ambiguous — refuse to write.
    Conflict { reason: String },
    /// Deliberately not acted on (e.g. a blocked deletion).
    Skipped { reason: String },
    /// Fewer than two members present — no peer to converge with.
    Noop,
}

/// A per-mount action produced by the decision and consumed by [`crate::execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Already at the winner OID.
    Skip { path: String },
    /// Back up the existing file before an overwrite (paired with `Write`/`Delete`).
    Backup { path: String },
    /// Overwrite an existing, differing member with the winner content.
    Write { path: String, from_oid: Oid },
    /// Create an absent member from the winner content.
    Create { path: String, from_oid: Oid },
    /// Remove a member (only under `--allow-delete`).
    Delete { path: String },
}

impl Action {
    /// The action verb used in reports.
    pub fn verb(&self) -> &'static str {
        match self {
            Action::Skip { .. } => "skip",
            Action::Backup { .. } => "backup",
            Action::Write { .. } => "write",
            Action::Create { .. } => "create",
            Action::Delete { .. } => "delete",
        }
    }
    pub fn path(&self) -> &str {
        match self {
            Action::Skip { path }
            | Action::Backup { path }
            | Action::Write { path, .. }
            | Action::Create { path, .. }
            | Action::Delete { path } => path,
        }
    }
}

/// The full group decision: which OID wins, why, and the per-mount actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub asset_id: String,
    pub winner_oid: Option<Oid>,
    pub kind: DecisionKind,
    pub actions: Vec<Action>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_ordering() {
        assert!(Recency::Now > Recency::At(i64::MAX));
        assert!(Recency::At(10) > Recency::At(5));
        assert!(Recency::At(i64::MIN) > Recency::NegInf);
        assert!(Recency::Now > Recency::NegInf);
    }

    #[test]
    fn asset_type_strings() {
        assert_eq!(AssetType::ClaudeMd.as_str(), "claude_md");
        assert_eq!(AssetType::Skill.as_str(), "skill");
    }
}
