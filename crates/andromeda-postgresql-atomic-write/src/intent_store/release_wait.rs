//! Waiting, on behalf of a mutation barrier, for another owner's lease on one
//! entity's queued intents to end.
//!
//! The holder either applies the intents, which removes them, or lets its
//! lease lapse, which makes them claimable; the wait polls for either on the
//! database clock that stamped the leases.

use crate::intent_store::{
    lease_statements::{ReleaseProbe, release_probe_statement},
    persistence,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use andromeda_temporal::{append_intent::TemporalIntentKey, error::TemporalError, port::temporal_intent_queue::EntityRelease};
use chrono::{DateTime, Utc};
use sea_query::Iden;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// How long the wait sleeps between probes at most.
///
/// It bounds the latency the barrier adds after a holder applies the intents
/// before its lease would have lapsed.
const RELEASE_POLL: Duration = Duration::from_millis(50);

/// Wait, at most `wait`, until no intent of `entity_id` keyed at most
/// `through` is held under a live lease.
pub(super) async fn await_release(pool: &PgPool, entity_id: &Identifier, through: TemporalIntentKey, wait: Duration) -> Result<EntityRelease, TemporalError> {
    let started = Instant::now();
    loop {
        let release = probe(pool, entity_id, through).await?;
        let EntityRelease::Held { remaining_lease } = release else {
            return Ok(release);
        };
        let left = wait.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Ok(release);
        }
        sleep(RELEASE_POLL.min(remaining_lease).min(left)).await;
    }
}

/// Read once what holds the intents of `entity_id` keyed at most `through`.
async fn probe(pool: &PgPool, entity_id: &Identifier, through: TemporalIntentKey) -> Result<EntityRelease, TemporalError> {
    let row = observe_statement(
        ObservedStatement::TemporalIntentReleaseProbe,
        release_probe_statement(entity_id, through).to_query().fetch_one(pool),
    )
    .await
    .map_err(|error| persistence(&error))?;
    let queued: i64 = row.try_get(ReleaseProbe::Queued.unquoted()).map_err(|error| persistence(&error))?;
    let claimable: i64 = row.try_get(ReleaseProbe::Claimable.unquoted()).map_err(|error| persistence(&error))?;
    let latest_lease: Option<DateTime<Utc>> = row.try_get(ReleaseProbe::LatestLease.unquoted()).map_err(|error| persistence(&error))?;
    let database_now: DateTime<Utc> = row.try_get(ReleaseProbe::DatabaseNow.unquoted()).map_err(|error| persistence(&error))?;
    release(queued, claimable, latest_lease, database_now)
}

/// What the probe's counts and clocks say of the intents it read.
///
/// The intents are claimable only when none of them is under a live lease: a
/// claim takes no intent behind a leased one of its Entity, so while any is
/// leased the barrier's claim comes back empty and only the wait can end it.
/// Every queued intent that is not claimable carries a live lease, so a probe
/// that counts one without reporting a lease breaks the statement's contract.
fn release(queued: i64, claimable: i64, latest_lease: Option<DateTime<Utc>>, database_now: DateTime<Utc>) -> Result<EntityRelease, TemporalError> {
    if queued == 0 {
        return Ok(EntityRelease::Clear);
    }
    if claimable == queued {
        return Ok(EntityRelease::Claimable);
    }
    match latest_lease {
        // A lease that lapsed between the comparison and the subtraction has
        // no time left to run.
        Some(latest_lease) => Ok(EntityRelease::Held {
            remaining_lease: (latest_lease - database_now).to_std().unwrap_or(Duration::ZERO),
        }),
        None => Err(TemporalError::from(PersistenceFailure::ContractBroken.record_bare())),
    }
}

#[cfg(test)]
mod tests {
    use crate::intent_store::release_wait::release;
    use andromeda_temporal::{error::TemporalError, port::temporal_intent_queue::EntityRelease};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant")
    }

    #[test]
    fn no_intent_left_up_to_the_mark_is_clear() {
        assert_eq!(release(0, 0, None, now()).expect("a probe outcome"), EntityRelease::Clear);
    }

    #[test]
    fn intents_none_of_which_is_leased_are_claimable() {
        assert_eq!(
            release(2, 2, Some(now() - TimeDelta::seconds(10)), now()).expect("a probe outcome"),
            EntityRelease::Claimable
        );
        assert_eq!(release(2, 2, None, now()).expect("a probe outcome"), EntityRelease::Claimable);
    }

    /// A claim takes nothing behind a leased intent of the Entity, so one
    /// live lease holds the rest back however many are unleased.
    #[test]
    fn one_leased_intent_holds_the_others_with_its_lease() {
        assert_eq!(
            release(2, 1, Some(now() + TimeDelta::seconds(10)), now()).expect("a probe outcome"),
            EntityRelease::Held {
                remaining_lease: Duration::from_secs(10)
            }
        );
    }

    #[test]
    fn intents_all_held_report_the_time_until_the_latest_lease_lapses() {
        assert_eq!(
            release(2, 0, Some(now() + TimeDelta::milliseconds(2300)), now()).expect("a probe outcome"),
            EntityRelease::Held {
                remaining_lease: Duration::from_millis(2300)
            }
        );
    }

    #[test]
    fn a_lease_that_lapsed_after_the_comparison_has_no_time_left() {
        assert_eq!(
            release(1, 0, Some(now() - TimeDelta::milliseconds(5)), now()).expect("a probe outcome"),
            EntityRelease::Held {
                remaining_lease: Duration::ZERO
            }
        );
    }

    #[test]
    fn held_intents_without_a_lease_break_the_probe_contract() {
        assert!(matches!(release(1, 0, None, now()), Err(TemporalError::Persistence(_))));
    }
}
