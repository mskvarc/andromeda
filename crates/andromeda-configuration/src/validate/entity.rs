//! The `[entity]` rules.

use crate::validate::{
    config_key::{ConfigKey, QueueName},
    durable_queue::queue_rules,
    rule::Rule,
};

/// The side-effect queue follows the queue rules. A dead-lettered job kept
/// for no time is removed the moment it gives up, and the removal and the
/// index rebuild need a positive cadence.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    queue_rules(QueueName::EntitySideEffects).into_iter().chain([
        Rule::Positive(ConfigKey::EntitySideEffectsDeadLetterAfter),
        Rule::Positive(ConfigKey::EntitySideEffectsRetentionSweepInterval),
        Rule::Positive(ConfigKey::EntitySpatialIndexRebuildInterval),
    ])
}

#[cfg(test)]
mod tests {
    use crate::validate::{config_key::ConfigKey, rule_tests::assert_positive_duration};

    #[test]
    fn a_zero_dead_letter_retention_cleanup_or_rebuild_interval_is_refused() {
        assert_positive_duration(
            |config, value| config.entity.side_effects.retention.dead_letter_after = value,
            ConfigKey::EntitySideEffectsDeadLetterAfter,
        );
        assert_positive_duration(
            |config, value| config.entity.side_effects.retention.sweep_interval = value,
            ConfigKey::EntitySideEffectsRetentionSweepInterval,
        );
        assert_positive_duration(
            |config, value| config.entity.spatial_index.rebuild_interval = value,
            ConfigKey::EntitySpatialIndexRebuildInterval,
        );
    }
}
