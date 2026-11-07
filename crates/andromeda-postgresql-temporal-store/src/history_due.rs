//! When a tenant's history next owes the conversion or the retention sweep
//! work.
//!
//! A chunk falls due for conversion once its range ended the columnstore
//! horizon ago, if the sweep would convert it (it is still in the rowstore and
//! not kept there), and only on an edition with a columnstore. A chunk falls
//! due for retention once its range ended the retention horizon ago, whatever
//! its storage, while retention is enabled. The tenant's history is due at the
//! earliest of those instants, which may lie in the past: a chunk a dormant
//! tenant never swept is overdue, and so is one a sweep cut short or deferred.
//!
//! Both instants come from the same chunk reads and the same selection the two
//! sweeps run, so a chunk the probe reports due is one a sweep acts on.

use crate::{
    columnstore_capability::ColumnstoreSupport,
    columnstore_conversion::{ChunkState, ColumnstoreHorizon, read_chunk_states, read_kept_chunks},
    retention_sweep::{TemporalRetention, TemporalRetentionHorizon, TemporalRetentionHorizonInvalid},
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgConnection;

/// The horizons the two history sweeps of every tenant run under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistorySchedule {
    /// How long after its range ends a chunk is converted, on an edition with
    /// a columnstore.
    pub columnstore: ColumnstoreHorizon,
    /// Whether and how long after its range ends a chunk is dropped.
    pub retention: TemporalRetention,
}

impl TryFrom<&TemporalStoragePolicy> for HistorySchedule {
    type Error = TemporalRetentionHorizonInvalid;

    fn try_from(policy: &TemporalStoragePolicy) -> Result<HistorySchedule, TemporalRetentionHorizonInvalid> {
        let retention = match policy.retention_after_days {
            None => TemporalRetention::Disabled,
            Some(days) => TemporalRetention::Enabled {
                horizon: TimeDelta::try_days(i64::from(days.get()))
                    .ok_or(TemporalRetentionHorizonInvalid)
                    .and_then(TemporalRetentionHorizon::new)?,
            },
        };
        Ok(HistorySchedule {
            columnstore: ColumnstoreHorizon::of_days(policy.columnstore_after_days),
            retention,
        })
    }
}

/// The earliest instant the history of the database behind `connection` owes
/// a sweep work, `None` when no chunk ever will under `schedule`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of the edition, chunk catalogue or kept-chunk
/// read.
pub async fn earliest_history_due(connection: &mut PgConnection, schedule: HistorySchedule) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let support = ColumnstoreSupport::detect(&mut *connection).await?;
    if support == ColumnstoreSupport::Absent && schedule.retention == TemporalRetention::Disabled {
        return Ok(None);
    }
    let states = read_chunk_states(connection).await?;
    let kept = match support {
        ColumnstoreSupport::Present => read_kept_chunks(connection).await?,
        ColumnstoreSupport::Absent => Vec::new(),
    };
    Ok(earliest_due(&states, &kept, support, schedule))
}

/// The earliest instant any of `states` falls due for conversion or
/// retention.
fn earliest_due(states: &[ChunkState], kept: &[DateTime<Utc>], support: ColumnstoreSupport, schedule: HistorySchedule) -> Option<DateTime<Utc>> {
    let conversion = states
        .iter()
        .filter(|state| match support {
            ColumnstoreSupport::Present => state.awaits_conversion(kept),
            ColumnstoreSupport::Absent => false,
        })
        .filter_map(|state| schedule.columnstore.due_at(state.range_end));
    let retention = states.iter().filter_map(|state| match schedule.retention {
        TemporalRetention::Enabled { horizon } => horizon.due_at(state.range_end),
        TemporalRetention::Disabled => None,
    });
    conversion.chain(retention).min()
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_capability::ColumnstoreSupport,
        columnstore_conversion::{ChunkState, ChunkStorage, ColumnstoreHorizon, HistoryChunk},
        history_due::{HistorySchedule, earliest_due},
        retention_sweep::{TemporalRetention, TemporalRetentionHorizon},
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_temporal::storage_policy::TemporalStoragePolicy;
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("an RFC 3339 instant").get()
    }

    /// A seven-day chunk ending at `range_end`.
    fn chunk(range_end: &str, storage: ChunkStorage) -> ChunkState {
        let end = instant(range_end);
        ChunkState {
            chunk: HistoryChunk {
                relation: "_timescaledb_internal._hyper_1_1_chunk".to_owned(),
                schema: "_timescaledb_internal".to_owned(),
                name: "_hyper_1_1_chunk".to_owned(),
                range_start: end - TimeDelta::days(7),
            },
            range_end: end,
            storage,
        }
    }

    /// Conversion after 7 days, and retention after 30 days when enabled.
    fn schedule(retention: Option<i64>) -> HistorySchedule {
        HistorySchedule {
            columnstore: ColumnstoreHorizon::of_days(NonZeroU32::new(7).expect("seven")),
            retention: retention.map_or(TemporalRetention::Disabled, |days| TemporalRetention::Enabled {
                horizon: TemporalRetentionHorizon::new(TimeDelta::days(days)).expect("a positive span"),
            }),
        }
    }

    #[test]
    fn the_oldest_rowstore_chunk_is_due_its_horizon_after_its_range_ends() {
        let states = [
            chunk("2026-09-10T00:00:00Z", ChunkStorage::Columnstore),
            chunk("2026-09-17T00:00:00Z", ChunkStorage::Rowstore),
            chunk("2026-09-24T00:00:00Z", ChunkStorage::Rowstore),
        ];
        assert_eq!(
            earliest_due(&states, &[], ColumnstoreSupport::Present, schedule(None)),
            Some(instant("2026-09-24T00:00:00Z"))
        );
    }

    #[test]
    fn a_chunk_kept_in_the_rowstore_never_falls_due_for_conversion() {
        let states = [
            chunk("2026-09-17T00:00:00Z", ChunkStorage::Rowstore),
            chunk("2026-09-24T00:00:00Z", ChunkStorage::Rowstore),
        ];
        let kept = [instant("2026-09-10T00:00:00Z")];
        assert_eq!(
            earliest_due(&states, &kept, ColumnstoreSupport::Present, schedule(None)),
            Some(instant("2026-10-01T00:00:00Z"))
        );
        let all_kept = [instant("2026-09-10T00:00:00Z"), instant("2026-09-17T00:00:00Z")];
        assert_eq!(earliest_due(&states, &all_kept, ColumnstoreSupport::Present, schedule(None)), None);
    }

    #[test]
    fn an_edition_without_a_columnstore_owes_no_conversion() {
        let states = [chunk("2026-09-17T00:00:00Z", ChunkStorage::Rowstore)];
        assert_eq!(earliest_due(&states, &[], ColumnstoreSupport::Absent, schedule(None)), None);
    }

    #[test]
    fn retention_is_due_for_the_oldest_chunk_whatever_its_storage_and_edition() {
        let states = [
            chunk("2026-08-01T00:00:00Z", ChunkStorage::Columnstore),
            chunk("2026-09-17T00:00:00Z", ChunkStorage::Rowstore),
        ];
        assert_eq!(
            earliest_due(&states, &[], ColumnstoreSupport::Present, schedule(Some(30))),
            Some(instant("2026-08-31T00:00:00Z"))
        );
        assert_eq!(
            earliest_due(&states, &[], ColumnstoreSupport::Absent, schedule(Some(30))),
            Some(instant("2026-08-31T00:00:00Z"))
        );
    }

    #[test]
    fn the_earlier_of_conversion_and_retention_is_the_history_s_due_instant() {
        let states = [chunk("2026-09-17T00:00:00Z", ChunkStorage::Rowstore)];
        assert_eq!(
            earliest_due(&states, &[], ColumnstoreSupport::Present, schedule(Some(3))),
            Some(instant("2026-09-20T00:00:00Z"))
        );
        assert_eq!(
            earliest_due(&states, &[], ColumnstoreSupport::Present, schedule(Some(30))),
            Some(instant("2026-09-24T00:00:00Z"))
        );
    }

    #[test]
    fn the_storage_policy_names_both_horizons() {
        let policy = |retention_after_days| TemporalStoragePolicy {
            chunk_interval_days: None,
            columnstore_after_days: NonZeroU32::new(7).expect("seven"),
            retention_after_days,
        };
        assert_eq!(HistorySchedule::try_from(&policy(None)), Ok(schedule(None)));
        assert_eq!(HistorySchedule::try_from(&policy(NonZeroU32::new(30))), Ok(schedule(Some(30))));
    }

    #[test]
    fn a_tenant_with_no_history_owes_no_sweep() {
        assert_eq!(earliest_due(&[], &[], ColumnstoreSupport::Present, schedule(Some(30))), None);
    }
}
