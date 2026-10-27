//! The `[temporal]` rules.

use crate::{
    schema::{
        feature_mode::FeatureMode,
        temporal::{
            query::{MAX_AGGREGATION_INSTANCES_CEILING, MAX_EDGE_GROUP_CEILING, MAX_LAST_N_CEILING},
            storage::TemporalStorageConfig,
        },
    },
    validate::{
        bound::{Quantity, Relation},
        config_key::{ConfigKey, QueueName},
        durable_queue::queue_rules,
        rule::{CompoundRule, Limit, Rule},
        violation::{ConfigurationViolation, Requirement},
        violation_log::ViolationLog,
    },
};

/// The query bounds, the history queue and the storage.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    query_rules().into_iter().chain(queue_rules(QueueName::TemporalHistory)).chain(storage_rules())
}

/// A read without `lastN` is paged at the default, which has to be a page a
/// client could ask for. A backward page holds up to `max_last_n` instances of
/// one attribute, all of which may share one instant, so the edge group bound
/// is at least that. The ceilings bound the work one request can ask for
/// however the bounds below them are tuned.
const fn query_rules() -> [Rule; 6] {
    [
        Rule::Positive(ConfigKey::TemporalQueryDefaultLastN),
        Rule::Ordered {
            key: ConfigKey::TemporalQueryMaxLastN,
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::TemporalQueryDefaultLastN),
        },
        Rule::Ordered {
            key: ConfigKey::TemporalQueryMaxLastN,
            relation: Relation::AtMost,
            limit: Limit::Fixed(Quantity::Count(MAX_LAST_N_CEILING as u64)),
        },
        Rule::Ordered {
            key: ConfigKey::TemporalQueryMaxEdgeGroup,
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::TemporalQueryMaxLastN),
        },
        Rule::Ordered {
            key: ConfigKey::TemporalQueryMaxEdgeGroup,
            relation: Relation::AtMost,
            limit: Limit::Fixed(Quantity::Count(MAX_EDGE_GROUP_CEILING as u64)),
        },
        Rule::Ordered {
            key: ConfigKey::TemporalQueryMaxAggregationInstances,
            relation: Relation::AtMost,
            limit: Limit::Fixed(Quantity::Count(MAX_AGGREGATION_INSTANCES_CEILING)),
        },
    ]
}

/// History partitions are day-grained, so every partition age is a whole
/// number of days, and a partition still receiving rows is never converted to
/// the columnstore. Every sweep needs a positive cadence.
const fn storage_rules() -> [Rule; 6] {
    [
        Rule::WholeDays(ConfigKey::TemporalStorageChunkInterval),
        Rule::WholeDays(ConfigKey::TemporalStorageConvertAfter),
        Rule::Ordered {
            key: ConfigKey::TemporalStorageConvertAfter,
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::TemporalStorageChunkInterval),
        },
        Rule::Positive(ConfigKey::TemporalStorageColumnstoreSweepInterval),
        Rule::WholeDays(ConfigKey::TemporalStorageDeleteAfter),
        Rule::Positive(ConfigKey::TemporalStorageRetentionSweepInterval),
    ]
}

/// What [`check_retention`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 1] = [CompoundRule {
    keys: &[ConfigKey::TemporalStorageDeleteAfter, ConfigKey::TemporalStorageRetentionMode],
    statement: "`temporal.storage.retention.delete_after` is required while `temporal.storage.retention.mode` is `\"enabled\"`.",
}];

/// Retention needs an age to delete at.
pub(crate) fn check_retention(storage: &TemporalStorageConfig, log: &mut ViolationLog) {
    if matches!(storage.retention.mode, FeatureMode::Enabled) && storage.retention.delete_after.is_none() {
        log.record(ConfigurationViolation::Unset {
            key: ConfigKey::TemporalStorageDeleteAfter,
            required_by: Requirement::Retention,
        });
    }
}

#[cfg(test)]
mod tests;
