//! The entity's own clause 4.8 system attributes in a notified entity.
//!
//! An entity state carries the system times of its attribute instances but not
//! the entity's own `createdAt` and `modifiedAt`, which the store keeps beside
//! it; a notification asking for `sysAttrs` (Table 5.2.14-1) has them written
//! onto the entity it carries, and a deletion has its `deletedAt` written there
//! too (clause 5.8.6). The typed walk writes them as it writes the entity
//! ([`StampedInstants`]).

use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_render::notification_entity::StampedInstants;
use chrono::{DateTime, Utc};

/// Whether the entity's expiry joins its system times.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpiryStamp {
    /// The entity's `expiresAt` is written when the entity carries none.
    Written,
    /// The entity's `expiresAt` is not part of the notification: an entity
    /// deletion is reduced to its identity and its system times.
    Omitted,
}

/// The entity times a notification writes on its entity: the system times
/// `instants` holds, when the subscription asked for them, and the deletion
/// instant of a deleted entity.
pub(crate) fn notified_instants(instants: Option<&SysAttrsSnapshot>, expiry: ExpiryStamp, deleted_at: Option<DateTime<Utc>>) -> StampedInstants {
    StampedInstants {
        created_at: instants.map(|instants| instants.created_at),
        modified_at: instants.map(|instants| instants.modified_at),
        expires_at: match expiry {
            ExpiryStamp::Written => instants.and_then(|instants| instants.expires_at),
            ExpiryStamp::Omitted => None,
        },
        deleted_at,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::processor::payload::entity_instants::{ExpiryStamp, notified_instants};
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn instants() -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant"),
            modified_at: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).single().expect("valid instant"),
            expires_at: Some(Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("valid instant")),
        }
    }

    /// Table 5.2.14-1: `sysAttrs` puts the entity's `createdAt`, `modifiedAt`
    /// and expiry on the notified entity.
    #[test]
    fn the_entity_system_times_are_stamped_with_its_expiry() {
        let stamped = notified_instants(Some(&instants()), ExpiryStamp::Written, None);
        assert_eq!(stamped.created_at, Some(instants().created_at));
        assert_eq!(stamped.modified_at, Some(instants().modified_at));
        assert_eq!(stamped.expires_at, instants().expires_at);
        assert_eq!(stamped.deleted_at, None);
    }

    /// An entity deletion is reduced to its identity and its system times.
    #[test]
    fn an_omitted_expiry_is_not_stamped() {
        let stamped = notified_instants(Some(&instants()), ExpiryStamp::Omitted, None);
        assert_eq!(stamped.expires_at, None);
        assert_eq!(stamped.created_at, Some(instants().created_at));
    }

    /// Clause 5.8.6: a deletion notification carries the deleted entity
    /// stamped with `deletedAt`, whether or not it asked for `sysAttrs`.
    #[test]
    fn a_deletion_is_stamped_with_deleted_at() {
        let deleted_at = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant");
        let stamped = notified_instants(None, ExpiryStamp::Omitted, Some(deleted_at));
        assert_eq!(stamped.deleted_at, Some(deleted_at));
        assert_eq!(stamped.created_at, None);
    }
}
