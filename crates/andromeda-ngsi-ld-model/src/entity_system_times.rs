//! The clause 4.8 system times of an entity as a peer wrote them.
//!
//! A context-independent entity state does not carry the entity-level
//! `createdAt`, `modifiedAt` and `deletedAt` of a representation received from
//! a peer, nor does the document a temporal evolution expands to, so the times
//! travel beside it.

use crate::entity::Entity;
use chrono::{DateTime, Utc};

/// The entity-level clause 4.8 system times a peer wrote, which the
/// context-independent state does not carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntitySystemTimes {
    /// The entity's `createdAt`.
    pub created_at: Option<DateTime<Utc>>,
    /// The entity's `modifiedAt`.
    pub modified_at: Option<DateTime<Utc>>,
    /// The entity's `deletedAt`, on a deletion notification.
    pub deleted_at: Option<DateTime<Utc>>,
}

impl EntitySystemTimes {
    /// The system times `entity` carries.
    #[must_use]
    pub const fn of<M>(entity: &Entity<M>) -> EntitySystemTimes {
        EntitySystemTimes {
            created_at: entity.created_at,
            modified_at: entity.modified_at,
            deleted_at: entity.deleted_at,
        }
    }

    /// Put the system times onto the compacted `entity`.
    pub const fn restore<M>(self, entity: &mut Entity<M>) {
        entity.created_at = self.created_at;
        entity.modified_at = self.modified_at;
        entity.deleted_at = self.deleted_at;
    }

    /// Whether none of the three times is set.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.created_at.is_none() && self.modified_at.is_none() && self.deleted_at.is_none()
    }

    /// These times, each one this holds and `other` is the first to lack
    /// filled from `other`: a merge keeps the first holder's times.
    #[must_use]
    pub fn or(self, other: EntitySystemTimes) -> EntitySystemTimes {
        EntitySystemTimes {
            created_at: self.created_at.or(other.created_at),
            modified_at: self.modified_at.or(other.modified_at),
            deleted_at: self.deleted_at.or(other.deleted_at),
        }
    }
}
