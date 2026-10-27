//! The `[entity]` rules.

use crate::{
    schema::entity::EntityConfig,
    validate::{
        config_key::{ConfigKey, QueueName},
        durable_queue::check_queue,
        violation_log::ViolationLog,
    },
};

/// The side-effect queue follows the queue rules. A dead-lettered job kept
/// for no time is removed the moment it gives up, and the removal and the
/// index rebuild need a positive cadence.
pub(crate) fn check_entity(entity: &EntityConfig, log: &mut ViolationLog) {
    check_queue(&entity.side_effects.queue, QueueName::EntitySideEffects, log);
    log.positive_duration(ConfigKey::EntitySideEffectsDeadLetterAfter, entity.side_effects.retention.dead_letter_after);
    log.positive_duration(ConfigKey::EntitySideEffectsCleanupInterval, entity.side_effects.retention.cleanup_interval);
    log.positive_duration(ConfigKey::EntitySpatialIndexRebuildInterval, entity.spatial_index.rebuild_interval);
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
            |config, value| config.entity.side_effects.retention.cleanup_interval = value,
            ConfigKey::EntitySideEffectsCleanupInterval,
        );
        assert_positive_duration(
            |config, value| config.entity.spatial_index.rebuild_interval = value,
            ConfigKey::EntitySpatialIndexRebuildInterval,
        );
    }
}
