//! The `[temporal]` rules.

use crate::{
    human_duration::HumanDuration,
    schema::{
        feature_mode::FeatureMode,
        temporal::{
            TemporalConfig,
            query::{MAX_AGGREGATION_INSTANCES_CEILING, MAX_EDGE_GROUP_CEILING, MAX_LAST_N_CEILING, TemporalQueryConfig},
            storage::{DEFAULT_CHUNK_INTERVAL, TemporalStorageConfig, is_positive_whole_days},
        },
    },
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::{ConfigKey, QueueName},
        durable_queue::check_queue,
        violation::{ConfigurationViolation, Requirement},
        violation_log::ViolationLog,
    },
};

/// The query bounds, the history queue and the storage.
pub(crate) fn check_temporal(temporal: &TemporalConfig, log: &mut ViolationLog) {
    check_query(&temporal.query, log);
    check_queue(&temporal.history.queue, QueueName::TemporalHistory, log);
    check_storage(&temporal.storage, log);
}

/// A read without `lastN` is paged at the default, which has to be a page a
/// client could ask for. A backward page holds up to `max_last_n` instances of
/// one attribute, all of which may share one instant, so the edge group bound
/// is at least that. The ceilings bound the work one request can ask for
/// however the bounds below them are tuned.
fn check_query(query: &TemporalQueryConfig, log: &mut ViolationLog) {
    log.positive_count(ConfigKey::TemporalQueryDefaultLastN, query.default_last_n.into());
    log.require(
        ConfigKey::TemporalQueryMaxLastN,
        u64::from(query.max_last_n),
        Relation::AtLeast,
        Bound::setting(ConfigKey::TemporalQueryDefaultLastN, u64::from(query.default_last_n)),
    );
    log.require(
        ConfigKey::TemporalQueryMaxLastN,
        u64::from(query.max_last_n),
        Relation::AtMost,
        Bound::Fixed(Quantity::Count(MAX_LAST_N_CEILING.into())),
    );
    log.require(
        ConfigKey::TemporalQueryMaxEdgeGroup,
        u64::from(query.max_edge_group),
        Relation::AtLeast,
        Bound::setting(ConfigKey::TemporalQueryMaxLastN, u64::from(query.max_last_n)),
    );
    log.require(
        ConfigKey::TemporalQueryMaxEdgeGroup,
        u64::from(query.max_edge_group),
        Relation::AtMost,
        Bound::Fixed(Quantity::Count(MAX_EDGE_GROUP_CEILING.into())),
    );
    log.require(
        ConfigKey::TemporalQueryMaxAggregationInstances,
        query.max_aggregation_instances.get(),
        Relation::AtMost,
        Bound::Fixed(Quantity::Count(MAX_AGGREGATION_INSTANCES_CEILING)),
    );
}

/// History partitions are day-grained, so every partition age is a whole
/// number of days, and a partition still receiving rows is never converted to
/// the columnstore. Retention needs an age to delete at, and every sweep a
/// positive cadence.
fn check_storage(storage: &TemporalStorageConfig, log: &mut ViolationLog) {
    if let Some(chunk_interval) = storage.chunk_interval {
        whole_days(ConfigKey::TemporalStorageChunkInterval, chunk_interval, log);
    }
    whole_days(ConfigKey::TemporalStorageConvertAfter, storage.columnstore.convert_after, log);
    log.require(
        ConfigKey::TemporalStorageConvertAfter,
        storage.columnstore.convert_after.duration(),
        Relation::AtLeast,
        Bound::setting(
            ConfigKey::TemporalStorageChunkInterval,
            storage.chunk_interval.unwrap_or(DEFAULT_CHUNK_INTERVAL).duration(),
        ),
    );
    log.positive_duration(ConfigKey::TemporalStorageColumnstoreSweepInterval, storage.columnstore.sweep_interval);
    match (storage.retention.mode, storage.retention.delete_after) {
        (FeatureMode::Enabled, None) => log.record(ConfigurationViolation::Unset {
            key: ConfigKey::TemporalStorageDeleteAfter,
            required_by: Requirement::Retention,
        }),
        (FeatureMode::Enabled | FeatureMode::Disabled, Some(delete_after)) => whole_days(ConfigKey::TemporalStorageDeleteAfter, delete_after, log),
        (FeatureMode::Disabled, None) => {}
    }
    log.positive_duration(ConfigKey::TemporalStorageRetentionSweepInterval, storage.retention.sweep_interval);
}

/// Record [`ConfigurationViolation::NotWholeDays`] unless `value` is a
/// positive whole number of days.
fn whole_days(key: ConfigKey, value: HumanDuration, log: &mut ViolationLog) {
    if !is_positive_whole_days(value) {
        log.record(ConfigurationViolation::NotWholeDays { key, value: value.duration() });
    }
}

#[cfg(test)]
mod tests;
