//! The entity state a matched subscription is filtered and notified with.
//!
//! A creation or an update hands its subscriptions the state the write left
//! behind, shared as it is. A deletion hands them the state it removed and,
//! beside it, the clause 4.8 system attributes captured before the row went:
//! by delivery time the row is gone and `createdAt` and `modifiedAt` can no
//! longer be read back (clause 5.8.6). The typed state holds no entity-level
//! system attributes, so they travel with the match instead of being embedded
//! in a copy of the state, and every match shares the one removed state.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use std::sync::Arc;

/// The entity state a matched subscription is filtered and notified with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SnapshotSupply<'a> {
    /// The mutation carries no rendered state.
    Absent,
    /// The state the mutation left behind, shared as it is.
    Ready(&'a Arc<EntityState>),
    /// A deletion's removed state, with the clause 4.8 system attributes the
    /// entity carried when it was removed.
    Removed {
        /// The state the deletion removed.
        removed: &'a Arc<EntityState>,
        /// The system attributes captured before the deletion, when they could
        /// be read.
        sys_attrs: Option<&'a SysAttrsSnapshot>,
    },
}

impl SnapshotSupply<'_> {
    /// The state a matched subscription is notified with, shared by every
    /// match of the change.
    pub(crate) fn materialize(self) -> Option<Arc<EntityState>> {
        match self {
            SnapshotSupply::Absent => None,
            SnapshotSupply::Ready(state) | SnapshotSupply::Removed { removed: state, .. } => Some(Arc::clone(state)),
        }
    }

    /// The clause 4.8 system attributes a deletion's match is notified with,
    /// which only a removed state carries.
    pub(crate) fn sys_attrs(self) -> Option<SysAttrsSnapshot> {
        match self {
            SnapshotSupply::Absent | SnapshotSupply::Ready(_) => None,
            // clone: three instants; every match owns its copy
            SnapshotSupply::Removed { sys_attrs, .. } => sys_attrs.cloned(),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::snapshot_supply::SnapshotSupply;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    /// A removed entity carrying one attribute instance.
    fn removed() -> Arc<EntityState> {
        let document = json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            "https://uri.etsi.org/ngsi-ld/default-context/category": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}]
            }]
        }]);
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity"))
    }

    /// The system attributes captured before the deletion.
    fn sys_attrs() -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("a valid instant"),
            modified_at: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).single().expect("a valid instant"),
            expires_at: None,
        }
    }

    #[test]
    fn a_removed_snapshot_without_system_attributes_is_the_shared_one() {
        let removed = removed();
        let supply = SnapshotSupply::Removed {
            removed: &removed,
            sys_attrs: None,
        };
        let materialized = supply.materialize().expect("a removed state");
        assert!(Arc::ptr_eq(&materialized, &removed));
        assert!(supply.sys_attrs().is_none());
    }

    #[test]
    fn a_ready_snapshot_is_the_shared_one() {
        let current = removed();
        let materialized = SnapshotSupply::Ready(&current).materialize().expect("a current state");
        assert!(Arc::ptr_eq(&materialized, &current));
        assert!(SnapshotSupply::Ready(&current).sys_attrs().is_none());
        assert!(SnapshotSupply::Absent.materialize().is_none());
    }

    /// Clause 5.8.6: the durable match of a deletion carries the removed
    /// state shared as it is and the clause 4.8 system attributes beside it.
    #[test]
    fn a_removed_snapshot_is_shared_and_carries_its_system_attributes_beside_it() {
        let removed = removed();
        let sys_attrs = sys_attrs();
        let supply = SnapshotSupply::Removed {
            removed: &removed,
            sys_attrs: Some(&sys_attrs),
        };
        let materialized = supply.materialize().expect("a removed state");
        assert!(Arc::ptr_eq(&materialized, &removed));
        pretty_assertions::assert_eq!(supply.sys_attrs().as_ref(), Some(&sys_attrs));
    }
}
