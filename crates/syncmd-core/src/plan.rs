//! The pure decision engine. **No I/O.** Given the discovered per-mount state, the baseline,
//! and the options, it returns a group-level [`Decision`] with per-mount [`Action`]s.
//!
//! This is the testability keystone: every branch is exercised with fabricated OIDs and
//! recencies, no git or filesystem required.

use crate::model::{
    Action, Asset, Baseline, Decision, DecisionKind, MountState, PlanOpts, Presence, Strategy,
    WinnerReason,
};

/// Classification of a mount relative to the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Present with content equal to the baseline (or, in bootstrap, never counted as changed).
    Unchanged,
    /// Present and differs from the baseline (edited or created).
    Edited,
    /// Absent now but existed at the baseline (a deletion).
    Deleted,
    /// Absent now and absent at the baseline — only ever a fill target.
    AbsentTarget,
}

fn classify(state: &MountState, baseline: &Baseline) -> Change {
    match state.presence {
        Presence::Present => match (&state.cur_oid, &baseline.oid) {
            // Bootstrap: any present member counts as a (created) change.
            (Some(_), None) => Change::Edited,
            (Some(cur), Some(base)) => {
                if cur == base {
                    Change::Unchanged
                } else {
                    Change::Edited
                }
            }
            (None, _) => Change::AbsentTarget, // present but unreadable OID — treat as target
        },
        Presence::Absent => {
            if state.present_at_baseline {
                Change::Deleted
            } else {
                Change::AbsentTarget
            }
        }
    }
}

/// Decide one group. `member_count` is the registry size (including absent members) so the
/// "all present" early-exit can require a full house.
pub fn plan_group(
    asset: &Asset,
    states: &[MountState],
    baseline: &Baseline,
    opts: PlanOpts,
) -> Decision {
    let member_count = asset.mounts.len().max(states.len());
    let present: Vec<&MountState> = states
        .iter()
        .filter(|s| s.presence == Presence::Present && s.cur_oid.is_some())
        .collect();

    // Fewer than two members present → no peer to converge with.
    if present.len() < 2 && !is_bootstrap_single(states, baseline) {
        // Still allow bootstrap (1 present + absent targets) below; otherwise no-op.
        if present.len() < 2 && !any_creatable(states, opts) {
            return noop(asset, states);
        }
    }

    // EARLY EXIT — already in sync: every registry member present AND all OIDs equal.
    if present.len() == member_count {
        let first = present[0].cur_oid.clone();
        if present.iter().all(|s| s.cur_oid == first) {
            return Decision {
                asset_id: asset.id.clone(),
                winner_oid: first,
                kind: DecisionKind::InSync,
                actions: states
                    .iter()
                    .map(|s| Action::Skip {
                        path: s.path().to_string(),
                    })
                    .collect(),
            };
        }
    }

    // CHANGED SET.
    let changed: Vec<&MountState> = states
        .iter()
        .filter(|s| matches!(classify(s, baseline), Change::Edited | Change::Deleted))
        .collect();

    // Bootstrap: no baseline ever existed.
    if baseline.is_bootstrap() {
        return bootstrap(asset, states, opts);
    }

    match changed.len() {
        0 => {
            // Nothing changed vs baseline, yet the early-exit didn't fire — so either a member
            // is missing (fill it) or the present members disagree (a contradiction guard).
            let first = present.first().and_then(|s| s.cur_oid.clone());
            let present_agree = present.iter().all(|s| s.cur_oid == first);
            if !present_agree {
                // Defensive: unreachable with a single baseline OID, but never guess.
                return Decision {
                    asset_id: asset.id.clone(),
                    winner_oid: None,
                    kind: DecisionKind::Conflict {
                        reason: "members differ but none changed since baseline".into(),
                    },
                    actions: skip_all(states),
                };
            }
            // Present members agree. Fill any absent creatable targets from the agreed content.
            let has_creatable =
                opts.create_missing && states.iter().any(|s| s.presence == Presence::Absent);
            if has_creatable {
                let winner = newest(asset, &present);
                propagate(asset, states, winner, WinnerReason::SingleChange, opts)
            } else {
                // Nothing actionable: everything present agrees and we won't create the rest.
                Decision {
                    asset_id: asset.id.clone(),
                    winner_oid: first,
                    kind: DecisionKind::InSync,
                    actions: skip_all(states),
                }
            }
        }
        1 => {
            let winner = changed[0];
            if classify(winner, baseline) == Change::Deleted {
                return resolve_deletion(asset, states, winner, opts);
            }
            propagate(asset, states, winner, WinnerReason::SingleChange, opts)
        }
        _ => resolve_divergence(asset, states, &changed, opts),
    }
}

/// Whether a single present member with absent creatable targets should bootstrap.
fn is_bootstrap_single(states: &[MountState], baseline: &Baseline) -> bool {
    baseline.is_bootstrap()
        && states
            .iter()
            .filter(|s| s.presence == Presence::Present)
            .count()
            >= 1
}

fn any_creatable(states: &[MountState], opts: PlanOpts) -> bool {
    opts.create_missing && states.iter().any(|s| s.presence == Presence::Absent)
}

fn noop(asset: &Asset, states: &[MountState]) -> Decision {
    Decision {
        asset_id: asset.id.clone(),
        winner_oid: None,
        kind: DecisionKind::Noop,
        actions: skip_all(states),
    }
}

fn skip_all(states: &[MountState]) -> Vec<Action> {
    states
        .iter()
        .map(|s| Action::Skip {
            path: s.path().to_string(),
        })
        .collect()
}

/// Pick the winner among `candidates` by recency, tie-broken by registry order (earlier wins).
fn newest<'a>(asset: &Asset, candidates: &[&'a MountState]) -> &'a MountState {
    let order = |path: &str| {
        asset
            .mounts
            .iter()
            .position(|m| m.path == path)
            .unwrap_or(usize::MAX)
    };
    candidates
        .iter()
        .copied()
        .max_by(|a, b| {
            a.recency
                .cmp(&b.recency)
                // Higher recency wins; on a tie, earlier registry position wins (so reverse).
                .then_with(|| order(b.path()).cmp(&order(a.path())))
        })
        .expect("non-empty candidate set")
}

fn bootstrap(asset: &Asset, states: &[MountState], opts: PlanOpts) -> Decision {
    let present: Vec<&MountState> = states
        .iter()
        .filter(|s| s.presence == Presence::Present && s.cur_oid.is_some())
        .collect();
    if present.is_empty() {
        return noop(asset, states);
    }
    let winner = newest(asset, &present);
    let mut d = propagate_core(asset, states, winner, opts);
    d.kind = DecisionKind::Bootstrap {
        winner_path: winner.path().to_string(),
    };
    d
}

fn propagate(
    asset: &Asset,
    states: &[MountState],
    winner: &MountState,
    reason: WinnerReason,
    opts: PlanOpts,
) -> Decision {
    let mut d = propagate_core(asset, states, winner, opts);
    d.kind = DecisionKind::Propagate {
        winner_path: winner.path().to_string(),
        reason,
    };
    d
}

/// Build the per-mount actions for propagating `winner`'s OID to the rest.
fn propagate_core(
    asset: &Asset,
    states: &[MountState],
    winner: &MountState,
    opts: PlanOpts,
) -> Decision {
    let winner_oid = winner.cur_oid.clone();
    let woid = winner_oid.clone().expect("winner has an OID");
    let mut actions = Vec::new();

    for s in states {
        if s.path() == winner.path() {
            actions.push(Action::Skip {
                path: s.path().to_string(),
            });
            continue;
        }
        match s.presence {
            Presence::Present => {
                if s.cur_oid.as_ref() == Some(&woid) {
                    actions.push(Action::Skip {
                        path: s.path().to_string(),
                    });
                } else {
                    if opts.backup {
                        actions.push(Action::Backup {
                            path: s.path().to_string(),
                        });
                    }
                    actions.push(Action::Write {
                        path: s.path().to_string(),
                        from_oid: woid.clone(),
                    });
                }
            }
            Presence::Absent => {
                if opts.create_missing {
                    actions.push(Action::Create {
                        path: s.path().to_string(),
                        from_oid: woid.clone(),
                    });
                } else {
                    actions.push(Action::Skip {
                        path: s.path().to_string(),
                    });
                }
            }
        }
    }

    Decision {
        asset_id: asset.id.clone(),
        winner_oid,
        kind: DecisionKind::Noop, // overwritten by caller
        actions,
    }
}

fn resolve_deletion(
    asset: &Asset,
    states: &[MountState],
    _winner: &MountState,
    opts: PlanOpts,
) -> Decision {
    if !opts.allow_delete {
        return Decision {
            asset_id: asset.id.clone(),
            winner_oid: None,
            kind: DecisionKind::Skipped {
                reason: "delete_blocked".into(),
            },
            actions: skip_all(states),
        };
    }
    // Propagate the deletion: remove every present member (backing up first).
    let mut actions = Vec::new();
    for s in states {
        match s.presence {
            Presence::Present => {
                if opts.backup {
                    actions.push(Action::Backup {
                        path: s.path().to_string(),
                    });
                }
                actions.push(Action::Delete {
                    path: s.path().to_string(),
                });
            }
            Presence::Absent => actions.push(Action::Skip {
                path: s.path().to_string(),
            }),
        }
    }
    Decision {
        asset_id: asset.id.clone(),
        winner_oid: None,
        kind: DecisionKind::Propagate {
            winner_path: _winner.path().to_string(),
            reason: WinnerReason::SingleChange,
        },
        actions,
    }
}

fn resolve_divergence(
    asset: &Asset,
    states: &[MountState],
    changed: &[&MountState],
    opts: PlanOpts,
) -> Decision {
    match opts.strategy {
        Strategy::Error | Strategy::Interactive => Decision {
            asset_id: asset.id.clone(),
            winner_oid: None,
            kind: DecisionKind::Conflict {
                reason: format!("{} members changed independently", changed.len()),
            },
            actions: skip_all(states),
        },
        Strategy::Newest => {
            let winner = newest(asset, changed);
            propagate(asset, states, winner, WinnerReason::Newest, opts)
        }
    }
}

/// Helper: the diverged losers (changed members that are not the winner) for reporting.
pub fn overridden_losers(decision: &Decision, states: &[MountState], baseline: &Baseline) -> Vec<String> {
    let winner = match &decision.kind {
        DecisionKind::Propagate { winner_path, .. } | DecisionKind::Bootstrap { winner_path } => {
            winner_path.clone()
        }
        _ => return Vec::new(),
    };
    states
        .iter()
        .filter(|s| {
            s.path() != winner && matches!(classify(s, baseline), Change::Edited | Change::Deleted)
        })
        .map(|s| s.path().to_string())
        .collect()
}

/// Convenience: was this group a real divergence (more than one changed member)?
pub fn was_divergence(states: &[MountState], baseline: &Baseline) -> bool {
    states
        .iter()
        .filter(|s| matches!(classify(s, baseline), Change::Edited | Change::Deleted))
        .count()
        > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssetMount, AssetType, MountType, Oid, Transform};

    fn oid(s: &str) -> Oid {
        Oid::new(s)
    }

    fn asset(paths: &[&str]) -> Asset {
        Asset {
            type_: AssetType::ClaudeMd,
            id: "test-id".into(),
            name: "instructions".into(),
            version: None,
            mounts: paths
                .iter()
                .map(|p| AssetMount::new(*p, "h"))
                .collect(),
        }
    }

    fn state(path: &str, cur: Option<&str>, at_base: bool, recency: crate::model::Recency) -> MountState {
        MountState {
            mount: AssetMount {
                type_: MountType::AssetMount,
                id: None,
                path: path.into(),
                harness: "h".into(),
                oid: cur.map(oid),
                transform: Transform::Identity,
            },
            presence: if cur.is_some() {
                Presence::Present
            } else {
                Presence::Absent
            },
            cur_oid: cur.map(oid),
            present_at_baseline: at_base,
            recency,
        }
    }

    use crate::model::Recency::*;

    #[test]
    fn in_sync_all_equal() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", Some("x"), true, At(1)),
            state("B", Some("x"), true, At(1)),
        ];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        assert_eq!(d.kind, DecisionKind::InSync);
        assert!(d.actions.iter().all(|a| matches!(a, Action::Skip { .. })));
    }

    #[test]
    fn single_change_propagates_with_backup() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", Some("y"), true, At(2)), // edited
            state("B", Some("x"), true, At(1)), // unchanged == baseline
        ];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        match d.kind {
            DecisionKind::Propagate { winner_path, reason } => {
                assert_eq!(winner_path, "A");
                assert_eq!(reason, WinnerReason::SingleChange);
            }
            other => panic!("expected propagate, got {other:?}"),
        }
        assert_eq!(d.winner_oid, Some(oid("y")));
        // B should be backed up then written.
        assert!(d.actions.contains(&Action::Backup { path: "B".into() }));
        assert!(d.actions.contains(&Action::Write {
            path: "B".into(),
            from_oid: oid("y")
        }));
    }

    #[test]
    fn divergence_newest_wins_and_lists_overridden() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", Some("ya"), true, At(5)),  // older
            state("B", Some("yb"), true, At(9)),  // newer → wins
        ];
        let base = Baseline::agreed(oid("x"));
        let d = plan_group(&a, &s, &base, PlanOpts::default());
        match &d.kind {
            DecisionKind::Propagate { winner_path, reason } => {
                assert_eq!(winner_path, "B");
                assert_eq!(*reason, WinnerReason::Newest);
            }
            other => panic!("expected propagate newest, got {other:?}"),
        }
        assert!(was_divergence(&s, &base));
        assert_eq!(overridden_losers(&d, &s, &base), vec!["A".to_string()]);
    }

    #[test]
    fn divergence_tie_breaks_on_registry_order() {
        let a = asset(&["A", "B"]); // A precedes B
        let s = vec![
            state("A", Some("ya"), true, Now), // tie on recency
            state("B", Some("yb"), true, Now),
        ];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        match d.kind {
            DecisionKind::Propagate { winner_path, .. } => assert_eq!(winner_path, "A"),
            other => panic!("expected A to win tie, got {other:?}"),
        }
    }

    #[test]
    fn divergence_error_strategy_conflicts() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", Some("ya"), true, At(5)),
            state("B", Some("yb"), true, At(9)),
        ];
        let opts = PlanOpts {
            strategy: Strategy::Error,
            ..PlanOpts::default()
        };
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), opts);
        assert!(matches!(d.kind, DecisionKind::Conflict { .. }));
        assert!(d.actions.iter().all(|a| matches!(a, Action::Skip { .. })));
    }

    #[test]
    fn bootstrap_creates_missing() {
        let a = asset(&["A", "B", "C"]);
        let s = vec![
            state("A", Some("x"), false, At(1)),
            state("B", None, false, NegInf),
            state("C", None, false, NegInf),
        ];
        let d = plan_group(&a, &s, &Baseline::bootstrap(), PlanOpts::default());
        match d.kind {
            DecisionKind::Bootstrap { winner_path } => assert_eq!(winner_path, "A"),
            other => panic!("expected bootstrap, got {other:?}"),
        }
        assert!(d.actions.contains(&Action::Create {
            path: "B".into(),
            from_oid: oid("x")
        }));
        assert!(d.actions.contains(&Action::Create {
            path: "C".into(),
            from_oid: oid("x")
        }));
    }

    #[test]
    fn subset_present_but_members_missing_is_not_in_sync() {
        // A,B present & equal; C absent → must fill C, not report in_sync.
        let a = asset(&["A", "B", "C"]);
        let s = vec![
            state("A", Some("x"), true, At(1)),
            state("B", Some("x"), true, At(1)),
            state("C", None, false, NegInf),
        ];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        assert_ne!(d.kind, DecisionKind::InSync);
        assert!(d.actions.contains(&Action::Create {
            path: "C".into(),
            from_oid: oid("x")
        }));
    }

    #[test]
    fn deletion_blocked_by_default() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", None, true, NegInf), // deleted (existed at baseline)
            state("B", Some("x"), true, At(1)),
        ];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        match d.kind {
            DecisionKind::Skipped { reason } => assert_eq!(reason, "delete_blocked"),
            other => panic!("expected skipped delete_blocked, got {other:?}"),
        }
    }

    #[test]
    fn deletion_allowed_propagates_delete() {
        let a = asset(&["A", "B"]);
        let s = vec![
            state("A", None, true, NegInf),
            state("B", Some("x"), true, At(1)),
        ];
        let opts = PlanOpts {
            allow_delete: true,
            ..PlanOpts::default()
        };
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), opts);
        assert!(matches!(d.kind, DecisionKind::Propagate { .. }));
        assert!(d.actions.contains(&Action::Delete { path: "B".into() }));
    }

    #[test]
    fn single_member_is_noop() {
        let a = asset(&["A"]);
        let s = vec![state("A", Some("x"), true, At(1))];
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), PlanOpts::default());
        assert_eq!(d.kind, DecisionKind::Noop);
    }

    #[test]
    fn agreeing_present_with_create_disabled_is_in_sync_noop() {
        // A,B present & equal, C absent, create_missing=false → nothing actionable.
        let a = asset(&["A", "B", "C"]);
        let s = vec![
            state("A", Some("x"), true, At(1)),
            state("B", Some("x"), true, At(1)),
            state("C", None, false, NegInf),
        ];
        let opts = PlanOpts {
            create_missing: false,
            ..PlanOpts::default()
        };
        let d = plan_group(&a, &s, &Baseline::agreed(oid("x")), opts);
        assert_eq!(d.kind, DecisionKind::InSync);
        assert!(d.actions.iter().all(|a| matches!(a, Action::Skip { .. })));
    }
}
