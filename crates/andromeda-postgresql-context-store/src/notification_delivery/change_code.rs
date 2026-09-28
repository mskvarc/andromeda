//! The numbers `notification_events` stores one entity change as.
//!
//! The domain vocabulary of what one write did -- the Entity's lifecycle
//! transition and each Attribute instance's transition -- and the
//! migration-owned registries that number it are variant-for-variant identical,
//! and the translation between them is written out by hand in both directions.
//! A blanket conversion keyed on the number would let a transition a later
//! revision adds pass silently into storage as some other one; written as
//! exhaustive matches, that addition fails to compile here, which is where the
//! decision about its persisted number belongs.

use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind};
use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode};

/// The registry member the schema persists for a lifecycle transition.
#[must_use]
pub const fn stored_lifecycle(lifecycle: EntityLifecycle) -> EntityLifecycleCode {
    match lifecycle {
        EntityLifecycle::Created => EntityLifecycleCode::Created,
        EntityLifecycle::Modified => EntityLifecycleCode::Modified,
        EntityLifecycle::Deleted => EntityLifecycleCode::Deleted,
    }
}

/// The lifecycle transition a stored registry member names.
#[must_use]
pub const fn lifecycle_of(code: EntityLifecycleCode) -> EntityLifecycle {
    match code {
        EntityLifecycleCode::Created => EntityLifecycle::Created,
        EntityLifecycleCode::Modified => EntityLifecycle::Modified,
        EntityLifecycleCode::Deleted => EntityLifecycle::Deleted,
    }
}

/// The registry member the schema persists for an instance transition.
#[must_use]
pub const fn stored_change_kind(kind: AttributeChangeKind) -> AttributeChangeKindCode {
    match kind {
        AttributeChangeKind::Created => AttributeChangeKindCode::Created,
        AttributeChangeKind::Updated => AttributeChangeKindCode::Updated,
        AttributeChangeKind::Deleted => AttributeChangeKindCode::Deleted,
    }
}

/// The instance transition a stored registry member names.
#[must_use]
pub const fn change_kind_of(code: AttributeChangeKindCode) -> AttributeChangeKind {
    match code {
        AttributeChangeKindCode::Created => AttributeChangeKind::Created,
        AttributeChangeKindCode::Updated => AttributeChangeKind::Updated,
        AttributeChangeKindCode::Deleted => AttributeChangeKind::Deleted,
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::change_code::{change_kind_of, lifecycle_of, stored_change_kind, stored_lifecycle};
    use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind};
    use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode};

    #[test]
    fn every_lifecycle_transition_survives_a_round_trip_through_its_stored_number() {
        for lifecycle in [EntityLifecycle::Created, EntityLifecycle::Modified, EntityLifecycle::Deleted] {
            pretty_assertions::assert_eq!(lifecycle_of(stored_lifecycle(lifecycle)), lifecycle);
        }
        pretty_assertions::assert_eq!(EntityLifecycleCode::ALL.len(), 3);
    }

    #[test]
    fn every_instance_transition_survives_a_round_trip_through_its_stored_number() {
        for kind in [AttributeChangeKind::Created, AttributeChangeKind::Updated, AttributeChangeKind::Deleted] {
            pretty_assertions::assert_eq!(change_kind_of(stored_change_kind(kind)), kind);
        }
        pretty_assertions::assert_eq!(AttributeChangeKindCode::ALL.len(), 3);
    }
}
