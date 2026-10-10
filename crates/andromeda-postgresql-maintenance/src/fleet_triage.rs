//! The order in which the fleet's tenants are visited.
//!
//! A pass visits one tenant, so the order decides how soon a degrading
//! database is sampled. It is decided from what the platform connection sees
//! without connecting to any tenant: each database's freeze pressure and
//! change counters from the fleet observation, and each tenant's pending work,
//! last visit and the counters recorded at its start from the platform tables.
//!
//! Churn only orders visits. `tup_updated + tup_deleted` also counts
//! hypertable deletes and queue updates that degrade no target; the probe
//! still decides whether to act. A tenant with no churn is skipped until the
//! staleness floor, which costs it nothing it needs: a breach run qualifies by
//! elapsed time, so an open run qualifies on the first breaching sample past
//! its duration however late that sample comes.

use crate::{
    fleet_observation::{ChangeCounters, DatabaseObservation, FleetObservation},
    freeze_policy::FreezePressure,
    state::{RegisteredTenant, TenantKey, tenant_inspection::InspectionRecord},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::cmp::Reverse;

/// Longest a tenant goes unvisited, churn or not.
const STALENESS_FLOOR: ChronoDuration = ChronoDuration::hours(24);

/// Shortest interval between two visits that freeze pressure or due work
/// earns a tenant. Neither clears on the visit itself (autovacuum freezes, and
/// a load gate can defer the action again), so without it one such tenant
/// would take every pass.
const URGENT_REVISIT_INTERVAL: ChronoDuration = ChronoDuration::minutes(5);

/// What triage knows about one active tenant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TenantStanding {
    /// The tenant's registry row and recorded inspection state.
    pub(crate) record: InspectionRecord,
    /// What the fleet observation saw of its database, `None` when the
    /// database does not exist.
    pub(crate) observed: Option<DatabaseObservation>,
    /// Whether a queued action is past its backoff. A rebuild cancelled
    /// mid-flight leaves its action pending, so its recovery artifact is
    /// reached through it.
    pub(crate) has_due_action: bool,
}

/// Why a tenant is due a visit, most urgent first.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum VisitReason {
    /// Its database's anti-wraparound vacuum is behind, the higher pressure
    /// first.
    FreezePressure(Reverse<FreezePressure>),
    /// A queued action waits.
    DueAction,
    /// It has gone unvisited for the staleness floor, or was never visited.
    Stale,
    /// Its database changed rows since its last visit started, the most
    /// changed first.
    Churn(Reverse<u64>),
}

/// Pair every active tenant with what the fleet observation saw of it and
/// whether it has due work.
pub(crate) fn standings(records: Vec<InspectionRecord>, fleet: &FleetObservation, due: &[TenantKey]) -> Vec<TenantStanding> {
    records
        .into_iter()
        .map(|record| TenantStanding {
            observed: fleet.database(record.tenant.key),
            has_due_action: due.contains(&record.tenant.key),
            record,
        })
        .collect()
}

/// The tenant to visit now, `None` when no tenant is due.
///
/// Ties within a reason go to the tenant visited longest ago, a tenant never
/// visited first.
pub(crate) fn select_tenant(standings: &[TenantStanding], now: DateTime<Utc>) -> Option<&RegisteredTenant> {
    standings
        .iter()
        .filter_map(|standing| {
            let reason = visit_reason(standing, now)?;
            Some(((reason, standing.record.last_inspected_at, standing.record.tenant.key), &standing.record.tenant))
        })
        .min_by_key(|(order, _)| *order)
        .map(|(_, tenant)| tenant)
}

/// Why `standing` is due a visit at `now`, `None` when it is not.
fn visit_reason(standing: &TenantStanding, now: DateTime<Utc>) -> Option<VisitReason> {
    let unvisited_for = |interval: ChronoDuration| {
        standing
            .record
            .last_inspected_at
            .is_none_or(|visited| now.signed_duration_since(visited) >= interval)
    };
    if unvisited_for(URGENT_REVISIT_INTERVAL) {
        let pressure = standing.observed.map_or(FreezePressure::Normal, |database| database.freeze.pressure());
        if pressure > FreezePressure::Normal {
            return Some(VisitReason::FreezePressure(Reverse(pressure)));
        }
        if standing.has_due_action {
            return Some(VisitReason::DueAction);
        }
    }
    if unvisited_for(STALENESS_FLOOR) {
        return Some(VisitReason::Stale);
    }
    let churn = churn_since(standing.observed?.changes, standing.record.recorded);
    (churn > 0).then_some(VisitReason::Churn(Reverse(churn)))
}

/// Rows changed between the `recorded` counters and the `observed` ones.
///
/// Statistics reset in between (a changed `stats_reset`, or a counter lower
/// than recorded, as after crash recovery) restart the count, so every change
/// since the reset counts; so does every change when nothing was recorded.
fn churn_since(observed: ChangeCounters, recorded: Option<ChangeCounters>) -> u64 {
    recorded
        .filter(|recorded| recorded.stats_reset == observed.stats_reset)
        .and_then(|recorded| observed.tuple_changes.checked_sub(recorded.tuple_changes))
        .unwrap_or(observed.tuple_changes)
}

#[cfg(test)]
mod tests {
    use crate::{
        fleet_observation::{ChangeCounters, DatabaseFreeze, DatabaseObservation},
        fleet_triage::{TenantStanding, select_tenant},
        freeze_policy::FreezeAge,
        state::{RegisteredTenant, TenantKey, tenant_inspection::InspectionRecord},
    };
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use pretty_assertions::assert_eq;

    const FREEZE_MAX_AGE: u64 = 200_000_000;

    /// A tenant last visited `visited_hours_ago`, whose database changed
    /// `changed` rows since then under unchanged statistics and is under no
    /// freeze pressure.
    fn standing(key: i64, now: DateTime<Utc>, visited_hours_ago: i64, changed: u64) -> TenantStanding {
        let recorded = ChangeCounters {
            tuple_changes: 1_000,
            stats_reset: None,
        };
        TenantStanding {
            record: InspectionRecord {
                tenant: RegisteredTenant {
                    key: TenantKey(key),
                    name: None,
                    database: TenantDatabaseName::from(format!("tenant_{key}")),
                },
                last_inspected_at: Some(now - ChronoDuration::hours(visited_hours_ago)),
                recorded: Some(recorded),
            },
            observed: Some(DatabaseObservation {
                tenant: TenantKey(key),
                freeze: DatabaseFreeze {
                    transaction: FreezeAge {
                        age: 0,
                        freeze_max_age: FREEZE_MAX_AGE,
                    },
                    multixact: FreezeAge {
                        age: 0,
                        freeze_max_age: FREEZE_MAX_AGE,
                    },
                },
                changes: ChangeCounters {
                    tuple_changes: recorded.tuple_changes + changed,
                    stats_reset: None,
                },
            }),
            has_due_action: false,
        }
    }

    /// `standing` with its database's transaction ID age at `age`.
    fn aged(mut standing: TenantStanding, age: u64) -> TenantStanding {
        standing.observed = standing.observed.map(|mut database| {
            database.freeze.transaction.age = age;
            database
        });
        standing
    }

    /// The tenants in the order successive passes visit them, each one's
    /// visit leaving the others' standing unchanged.
    fn visit_order(mut standings: Vec<TenantStanding>, now: DateTime<Utc>) -> Vec<i64> {
        let mut order = Vec::new();
        while let Some(key) = select_tenant(&standings, now).map(|tenant| tenant.key) {
            order.push(key.0);
            standings.retain(|standing| standing.record.tenant.key != key);
        }
        order
    }

    fn selected(standings: &[TenantStanding], now: DateTime<Utc>) -> Option<i64> {
        select_tenant(standings, now).map(|tenant| tenant.key.0)
    }

    #[test]
    fn tenants_without_churn_are_skipped() {
        let now = Utc::now();
        let standings = vec![standing(1, now, 1, 0), standing(2, now, 2, 0), standing(3, now, 3, 500)];
        assert_eq!(visit_order(standings, now), vec![3]);
    }

    #[test]
    fn stats_reset_counts_as_churn() {
        let now = Utc::now();
        let mut reset = standing(1, now, 1, 0);
        reset.observed = reset.observed.map(|mut database| {
            database.changes = ChangeCounters {
                tuple_changes: 10,
                stats_reset: Some(now - ChronoDuration::minutes(30)),
            };
            database
        });
        assert_eq!(selected(&[reset, standing(2, now, 1, 0)], now), Some(1));
    }

    #[test]
    fn a_counter_that_went_backwards_counts_as_churn() {
        let now = Utc::now();
        let mut recovered = standing(1, now, 1, 0);
        recovered.observed = recovered.observed.map(|mut database| {
            database.changes.tuple_changes = 10;
            database
        });
        assert_eq!(selected(&[recovered], now), Some(1));
    }

    #[test]
    fn staleness_floor_outranks_churn() {
        let now = Utc::now();
        let busy = standing(1, now, 1, 10_000);
        let stale = standing(2, now, 25, 0);
        assert_eq!(visit_order(vec![busy, stale], now), vec![2, 1]);
    }

    #[test]
    fn freeze_pressure_and_pending_work_outrank_everything() {
        let now = Utc::now();
        let mut pending = standing(2, now, 1, 0);
        pending.has_due_action = true;
        let standings = vec![
            standing(4, now, 1, 10_000),
            standing(3, now, 25, 0),
            pending,
            aged(standing(1, now, 1, 0), FREEZE_MAX_AGE * 5 / 4),
        ];
        assert_eq!(visit_order(standings, now), vec![1, 2, 3, 4]);
    }

    #[test]
    fn critical_freeze_pressure_outranks_overdue() {
        let now = Utc::now();
        let overdue = aged(standing(1, now, 1, 0), FREEZE_MAX_AGE * 5 / 4);
        let critical = aged(standing(2, now, 1, 0), 1_000_000_000);
        assert_eq!(visit_order(vec![overdue, critical], now), vec![2, 1]);
    }

    #[test]
    fn urgent_tenants_wait_for_the_revisit_interval() {
        let now = Utc::now();
        let mut pressured = aged(standing(1, now, 0, 0), FREEZE_MAX_AGE * 5 / 4);
        pressured.record.last_inspected_at = Some(now - ChronoDuration::minutes(4));
        let mut pending = standing(2, now, 0, 0);
        pending.has_due_action = true;
        pending.record.last_inspected_at = Some(now - ChronoDuration::minutes(4));
        let standings = [pressured, pending];
        assert_eq!(selected(&standings, now), None);
        assert_eq!(selected(&standings, now + ChronoDuration::minutes(1)), Some(1));
    }

    #[test]
    fn churn_ranks_the_most_changed_tenant_first_and_ties_go_to_the_longest_unvisited() {
        let now = Utc::now();
        let standings = vec![standing(1, now, 1, 100), standing(2, now, 2, 100), standing(3, now, 1, 5_000)];
        assert_eq!(visit_order(standings, now), vec![3, 2, 1]);
    }

    #[test]
    fn a_tenant_never_visited_is_due_at_once() {
        let now = Utc::now();
        let mut fresh = standing(1, now, 0, 0);
        fresh.record.last_inspected_at = None;
        fresh.record.recorded = None;
        assert_eq!(visit_order(vec![standing(2, now, 1, 10_000), fresh], now), vec![1, 2]);
    }

    #[test]
    fn a_tenant_whose_database_is_missing_waits_for_the_staleness_floor() {
        let now = Utc::now();
        let mut missing = standing(1, now, 23, 0);
        missing.observed = None;
        let standings = [missing];
        assert_eq!(selected(&standings, now), None);
        assert_eq!(selected(&standings, now + ChronoDuration::hours(1)), Some(1));
    }
}
