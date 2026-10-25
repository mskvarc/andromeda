//! Removing the attribute instances whose `expiresAt` has passed (clause
//! 4.22).

use crate::entity_state::EntityState;
use chrono::{DateTime, Utc};
use std::borrow::Cow;

/// Whether removing expired instances removed any.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceExpiry {
    /// At least one instance had expired and was removed.
    Removed,
    /// No instance had expired; the entity is unchanged.
    NoneExpired,
}

impl EntityState {
    /// Remove every attribute instance whose `expiresAt` is at or before
    /// `at`, and every attribute left with no instance.
    ///
    /// Clause 4.22 gives an instance-level `expiresAt` the meaning an
    /// entity-level one has: once it passes, the instance is gone. This is
    /// [`EntityState::standing_at`] made the entity's own state: only the
    /// attributes holding an expired instance are copied.
    pub fn remove_expired_instances(&mut self, at: DateTime<Utc>) -> InstanceExpiry {
        let standing = match self.standing_at(at) {
            Cow::Borrowed(_) => return InstanceExpiry::NoneExpired,
            Cow::Owned(standing) => standing,
        };
        *self = standing;
        InstanceExpiry::Removed
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity_state::EntityState,
        instance::Instance,
        operation::{
            expiry_removal::InstanceExpiry,
            speed_entity::{dataset_ids, date_time, entity, instances, number, property},
        },
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use std::sync::Arc;

    const SPEED: &str = "https://example.org/speed";

    fn cutoff() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("cutoff parses").get()
    }

    fn instance(expires_at: Option<&str>) -> Instance {
        let mut instance = property(number(1));
        instance.set_expires_at(expires_at.map(date_time));
        instance
    }

    fn entity_with(expires_at: Option<&str>) -> EntityState {
        entity(vec![(SPEED, instances(vec![(None, instance(expires_at))]))])
    }

    #[test]
    fn an_instance_past_its_expiry_is_removed() {
        let mut target = entity_with(Some("2025-12-31T23:59:59Z"));
        pretty_assertions::assert_eq!(target.remove_expired_instances(cutoff()), InstanceExpiry::Removed);
        assert!(target.attributes().is_empty());
    }

    #[test]
    fn an_instance_expiring_exactly_at_the_cutoff_is_removed() {
        let mut target = entity_with(Some("2026-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(target.remove_expired_instances(cutoff()), InstanceExpiry::Removed);
        assert!(target.attributes().is_empty());
    }

    #[test]
    fn an_instance_expiring_after_the_cutoff_survives() {
        let mut target = entity_with(Some("2026-01-01T00:00:01Z"));
        pretty_assertions::assert_eq!(target.remove_expired_instances(cutoff()), InstanceExpiry::NoneExpired);
        assert!(target.attributes().contains(SPEED));
    }

    #[test]
    fn an_instance_with_no_expiry_survives() {
        let mut target = entity_with(None);
        pretty_assertions::assert_eq!(target.remove_expired_instances(cutoff()), InstanceExpiry::NoneExpired);
        assert!(target.attributes().contains(SPEED));
    }

    #[test]
    fn only_the_expired_instance_of_a_multi_attribute_goes() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![
                (Some("urn:d1"), instance(Some("2025-06-01T00:00:00Z"))),
                (Some("urn:d2"), instance(Some("2026-06-01T00:00:00Z"))),
            ]),
        )]);
        pretty_assertions::assert_eq!(target.remove_expired_instances(cutoff()), InstanceExpiry::Removed);
        pretty_assertions::assert_eq!(dataset_ids(&target, SPEED), vec![Some("urn:d2".to_owned())]);
    }

    #[test]
    fn an_attribute_whose_last_instance_expires_loses_its_key() {
        let mut target = entity_with(Some("2025-01-01T00:00:00Z"));
        target.remove_expired_instances(cutoff());
        assert!(!target.attributes().contains(SPEED));
    }

    /// The entity's own members are not attribute instances and stay.
    #[test]
    fn the_entity_identity_and_expiry_are_preserved() {
        let mut target = entity_with(Some("2025-01-01T00:00:00Z"));
        target.set_expires_at(Some(date_time("2025-01-01T00:00:00Z")));
        let before = target.clone();
        target.remove_expired_instances(cutoff());
        pretty_assertions::assert_eq!(target.id(), before.id());
        pretty_assertions::assert_eq!(target.types(), before.types());
        pretty_assertions::assert_eq!(target.expires_at(), before.expires_at());
    }

    /// An attribute holding no expired instance is not copied.
    #[test]
    fn an_attribute_without_an_expired_instance_is_not_copied() {
        let mut target = entity(vec![
            (SPEED, instances(vec![(None, instance(Some("2025-01-01T00:00:00Z")))])),
            ("https://example.org/brand", instances(vec![(None, instance(None))])),
        ]);
        let before = target.clone();
        target.remove_expired_instances(cutoff());
        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared("https://example.org/brand").expect("brand"));
        assert!(Arc::ptr_eq(&shared(&before), &shared(&target)));
    }
}
