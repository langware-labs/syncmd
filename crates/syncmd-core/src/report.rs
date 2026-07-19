//! Structured, flow-sdk-shaped output types.
//!
//! Same serde policy as [`crate::model`]: `type` first, `id` next (omitted if `None`),
//! snake_case fields, `None` omitted.

use serde::{Deserialize, Serialize};

use crate::model::{Action, Asset, Decision, DecisionKind, WinnerReason};

/// Stable status string for a group, derived from [`DecisionKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupStatus {
    InSync,
    Propagated,
    DivergedResolved,
    Conflict,
    Skipped,
    Noop,
}

/// The `type` discriminator for the top-level report record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportType {
    SyncReport,
}

/// Per-mount line in a group report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountReport {
    pub path: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_oid: Option<String>,
    /// `false` under `--dry-run` (the action was planned but not applied).
    pub applied: bool,
}

/// One equivalence group's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupReport {
    #[serde(rename = "type")]
    pub type_: crate::model::AssetType,
    pub id: String,
    pub name: String,
    pub status: GroupStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_reason: Option<WinnerReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overridden: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub mounts: Vec<MountReport>,
}

/// Run-level rollup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub groups: usize,
    pub in_sync: usize,
    pub propagated: usize,
    pub conflicts: usize,
    pub skipped: usize,
    pub written: usize,
}

/// The top-level structured result returned by [`crate::plan`] and [`crate::sync`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    #[serde(rename = "type")]
    pub type_: ReportType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub root: String,
    pub groups: Vec<GroupReport>,
    pub summary: Summary,
}

impl SyncReport {
    pub fn new(root: impl Into<String>) -> Self {
        SyncReport {
            type_: ReportType::SyncReport,
            id: None,
            root: root.into(),
            groups: Vec::new(),
            summary: Summary::default(),
        }
    }

    /// Recompute the summary from the current groups.
    pub fn finalize(&mut self) {
        let mut s = Summary {
            groups: self.groups.len(),
            ..Default::default()
        };
        for g in &self.groups {
            match g.status {
                GroupStatus::InSync => s.in_sync += 1,
                GroupStatus::Propagated | GroupStatus::DivergedResolved => s.propagated += 1,
                GroupStatus::Conflict => s.conflicts += 1,
                GroupStatus::Skipped => s.skipped += 1,
                GroupStatus::Noop => {}
            }
            s.written += g
                .mounts
                .iter()
                .filter(|m| m.applied && matches!(m.action.as_str(), "write" | "create" | "delete"))
                .count();
        }
        self.summary = s;
    }

    /// Exit code for the whole run: 1 if any group is an unresolved conflict, else 0.
    pub fn exit_code(&self) -> i32 {
        if self.summary.conflicts > 0 {
            1
        } else {
            0
        }
    }
}

impl GroupStatus {
    /// Map a decision (and whether divergence was involved) to a stable status.
    pub fn from_decision(kind: &DecisionKind, diverged: bool) -> GroupStatus {
        match kind {
            DecisionKind::InSync => GroupStatus::InSync,
            DecisionKind::Bootstrap { .. } => GroupStatus::Propagated,
            DecisionKind::Propagate { .. } => {
                if diverged {
                    GroupStatus::DivergedResolved
                } else {
                    GroupStatus::Propagated
                }
            }
            DecisionKind::Conflict { .. } => GroupStatus::Conflict,
            DecisionKind::Skipped { .. } => GroupStatus::Skipped,
            DecisionKind::Noop => GroupStatus::Noop,
        }
    }
}

/// Build a [`GroupReport`] from an asset + its decision. `applied` marks whether actions were
/// actually executed (false for dry-run / plan). `overridden` lists diverged losers.
pub fn group_report(
    asset: &Asset,
    decision: &Decision,
    baseline: &crate::model::Baseline,
    diverged: bool,
    overridden: Vec<String>,
    note: Option<String>,
    applied: bool,
) -> GroupReport {
    let (winner_path, winner_reason) = match &decision.kind {
        DecisionKind::Propagate { winner_path, reason } => {
            (Some(winner_path.clone()), Some(*reason))
        }
        DecisionKind::Bootstrap { winner_path } => {
            (Some(winner_path.clone()), Some(WinnerReason::Bootstrap))
        }
        _ => (None, None),
    };

    let mounts = decision
        .actions
        .iter()
        .map(|a| MountReport {
            path: a.path().to_string(),
            action: a.verb().to_string(),
            from_oid: match a {
                Action::Write { from_oid, .. } | Action::Create { from_oid, .. } => {
                    Some(from_oid.to_string())
                }
                _ => None,
            },
            applied: applied && !matches!(a, Action::Skip { .. }),
        })
        .collect();

    GroupReport {
        type_: asset.type_,
        id: asset.id.clone(),
        name: asset.name.clone(),
        status: GroupStatus::from_decision(&decision.kind, diverged),
        baseline: baseline.oid.as_ref().map(|o| o.to_string()),
        winner_path,
        winner_reason,
        winner_oid: decision.winner_oid.as_ref().map(|o| o.to_string()),
        overridden,
        note,
        mounts,
    }
}
