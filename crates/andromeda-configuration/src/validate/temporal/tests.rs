use crate::{
    broker_config::BrokerConfig,
    human_duration::HumanDuration,
    schema::feature_mode::FeatureMode,
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::ConfigKey,
        rule_tests::{assert_accepted, assert_positive_duration, refusals},
        violation::{ConfigurationViolation, Requirement},
    },
};
use pretty_assertions::assert_eq;
use std::{num::NonZeroU64, time::Duration};

/// One day, in seconds.
const DAY: u64 = 86_400;

/// A count rule's refusal of `value` against `bound`.
fn count_out_of_range(key: ConfigKey, value: u64, relation: Relation, bound: Bound) -> ConfigurationViolation {
    ConfigurationViolation::OutOfRange {
        key,
        value: Quantity::Count(value),
        relation,
        bound,
    }
}

#[test]
fn a_zero_default_last_n_is_refused_and_one_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.default_last_n = 0;
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::Zero {
            key: ConfigKey::TemporalQueryDefaultLastN
        }]
    );
    config.temporal.query.default_last_n = 1;
    assert_accepted(&config);
}

#[test]
fn a_max_last_n_below_the_default_is_refused_and_an_equal_one_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.default_last_n = 100;
    config.temporal.query.max_last_n = 99;
    assert_eq!(
        refusals(&config),
        [count_out_of_range(
            ConfigKey::TemporalQueryMaxLastN,
            99,
            Relation::AtLeast,
            Bound::setting(ConfigKey::TemporalQueryDefaultLastN, 100_u64)
        )]
    );
    config.temporal.query.max_last_n = 100;
    assert_accepted(&config);
}

#[test]
fn a_max_last_n_past_its_ceiling_is_refused_and_the_ceiling_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.max_last_n = 5_001;
    assert_eq!(
        refusals(&config),
        [count_out_of_range(
            ConfigKey::TemporalQueryMaxLastN,
            5_001,
            Relation::AtMost,
            Bound::Fixed(Quantity::Count(5_000))
        )]
    );
    config.temporal.query.max_last_n = 5_000;
    assert_accepted(&config);
}

#[test]
fn an_edge_group_below_the_max_last_n_is_refused_and_an_equal_one_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.max_edge_group = 999;
    assert_eq!(
        refusals(&config),
        [count_out_of_range(
            ConfigKey::TemporalQueryMaxEdgeGroup,
            999,
            Relation::AtLeast,
            Bound::setting(ConfigKey::TemporalQueryMaxLastN, 1_000_u64)
        )]
    );
    config.temporal.query.max_edge_group = 1_000;
    assert_accepted(&config);
}

#[test]
fn an_edge_group_past_its_ceiling_is_refused_and_the_ceiling_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.max_edge_group = 1_000_001;
    assert_eq!(
        refusals(&config),
        [count_out_of_range(
            ConfigKey::TemporalQueryMaxEdgeGroup,
            1_000_001,
            Relation::AtMost,
            Bound::Fixed(Quantity::Count(1_000_000))
        )]
    );
    config.temporal.query.max_edge_group = 1_000_000;
    assert_accepted(&config);
}

#[test]
fn an_aggregation_bound_past_its_ceiling_is_refused_and_the_ceiling_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.query.max_aggregation_instances = NonZeroU64::MIN.saturating_add(100_000_000);
    assert_eq!(
        refusals(&config),
        [count_out_of_range(
            ConfigKey::TemporalQueryMaxAggregationInstances,
            100_000_001,
            Relation::AtMost,
            Bound::Fixed(Quantity::Count(100_000_000))
        )]
    );
    config.temporal.query.max_aggregation_instances = NonZeroU64::MIN.saturating_add(99_999_999);
    assert_accepted(&config);
}

#[test]
fn a_chunk_interval_of_part_days_is_refused_and_whole_days_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.storage.chunk_interval = Some(HumanDuration::from_secs(DAY + DAY / 2));
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::NotWholeDays {
            key: ConfigKey::TemporalStorageChunkInterval,
            value: Duration::from_secs(DAY + DAY / 2),
        }]
    );
    config.temporal.storage.chunk_interval = Some(HumanDuration::from_days(2));
    assert_accepted(&config);
}

#[test]
fn a_columnstore_lag_of_part_days_is_refused_and_whole_days_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.storage.columnstore.convert_after = HumanDuration::from_secs(8 * DAY + DAY / 2);
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::NotWholeDays {
            key: ConfigKey::TemporalStorageConvertAfter,
            value: Duration::from_secs(8 * DAY + DAY / 2),
        }]
    );
    config.temporal.storage.columnstore.convert_after = HumanDuration::from_days(8);
    assert_accepted(&config);
}

/// Without an override the bound is the schema's own seven-day partition.
#[test]
fn a_columnstore_lag_below_the_chunk_interval_is_refused_and_an_equal_one_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.storage.columnstore.convert_after = HumanDuration::from_days(6);
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::TemporalStorageConvertAfter,
            value: Quantity::Duration(Duration::from_secs(6 * DAY)),
            relation: Relation::AtLeast,
            bound: Bound::setting(ConfigKey::TemporalStorageChunkInterval, Duration::from_secs(7 * DAY)),
        }]
    );
    config.temporal.storage.columnstore.convert_after = HumanDuration::from_days(7);
    assert_accepted(&config);
    config.temporal.storage.chunk_interval = Some(HumanDuration::from_days(8));
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::TemporalStorageConvertAfter,
            value: Quantity::Duration(Duration::from_secs(7 * DAY)),
            relation: Relation::AtLeast,
            bound: Bound::setting(ConfigKey::TemporalStorageChunkInterval, Duration::from_secs(8 * DAY)),
        }]
    );
}

#[test]
fn a_zero_columnstore_or_retention_sweep_is_refused() {
    assert_positive_duration(
        |config, value| config.temporal.storage.columnstore.sweep_interval = value,
        ConfigKey::TemporalStorageColumnstoreSweepInterval,
    );
    assert_positive_duration(
        |config, value| config.temporal.storage.retention.sweep_interval = value,
        ConfigKey::TemporalStorageRetentionSweepInterval,
    );
}

#[test]
fn enabled_retention_without_an_age_is_refused_and_with_one_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.storage.retention.mode = FeatureMode::Enabled;
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::Unset {
            key: ConfigKey::TemporalStorageDeleteAfter,
            required_by: Requirement::Retention,
        }]
    );
    config.temporal.storage.retention.delete_after = Some(HumanDuration::from_days(365));
    assert_accepted(&config);
}

/// The age is read only while retention is enabled, yet a part-day age is
/// refused either way, so turning retention on never surfaces it later.
#[test]
fn a_retention_age_of_part_days_is_refused_and_one_day_accepted() {
    let mut config = BrokerConfig::default();
    config.temporal.storage.retention.delete_after = Some(HumanDuration::from_secs(DAY / 2));
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::NotWholeDays {
            key: ConfigKey::TemporalStorageDeleteAfter,
            value: Duration::from_secs(DAY / 2),
        }]
    );
    config.temporal.storage.retention.delete_after = Some(HumanDuration::from_days(1));
    assert_accepted(&config);
}
