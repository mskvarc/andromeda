//! The entity state a matched subscription is filtered and notified with.
//!
//! A creation or an update hands its subscriptions the state the write left
//! behind; a deletion hands them the state it removed. Either travels with the
//! clause 4.8 system attributes the Entity carried in that state: a
//! notification reports the change as it left the Entity (clause 5.8.6), so it
//! is stamped with that change's `createdAt` and `modifiedAt`, never with the
//! ones a later write moved the row to, and a deleted row cannot be read back
//! at all. The typed state holds no entity-level system attributes, so they
//! travel beside it instead of being embedded in a copy of the state, and every
//! match shares the one state.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use std::sync::Arc;

/// The entity state a matched subscription is filtered and notified with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SnapshotSupply<'a> {
    /// The mutation carries no rendered state.
    Absent,
    /// The state the mutation left behind, or for a deletion the state it
    /// removed, with the clause 4.8 system attributes the Entity carried in it.
    Carried {
        /// The state, shared as it is.
        state: &'a Arc<EntityState>,
        /// The Entity's system attributes in `state`, `None` when the change
        /// does not know them.
        sys_attrs: Option<&'a SysAttrsSnapshot>,
    },
}

impl SnapshotSupply<'_> {
    /// The state a matched subscription is notified with, shared by every
    /// match of the change.
    pub(crate) fn materialize(self) -> Option<Arc<EntityState>> {
        match self {
            SnapshotSupply::Absent => None,
            SnapshotSupply::Carried { state, .. } => Some(Arc::clone(state)),
        }
    }

    /// The clause 4.8 system attributes a match of the change is notified
    /// with.
    pub(crate) fn sys_attrs(self) -> Option<SysAttrsSnapshot> {
        match self {
            SnapshotSupply::Absent => None,
            // clone: three instants; every match owns its copy
            SnapshotSupply::Carried { sys_attrs, .. } => sys_attrs.cloned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot_supply::SnapshotSupply;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    /// An entity carrying one attribute instance.
    fn building() -> Arc<EntityState> {
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

    fn sys_attrs() -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("a valid instant"),
            modified_at: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).single().expect("a valid instant"),
            expires_at: None,
        }
    }

    #[test]
    fn a_carried_state_without_system_attributes_is_the_shared_one() {
        let state = building();
        let supply = SnapshotSupply::Carried {
            state: &state,
            sys_attrs: None,
        };
        let materialized = supply.materialize().expect("a carried state");
        assert!(Arc::ptr_eq(&materialized, &state));
        assert!(supply.sys_attrs().is_none());
    }

    #[test]
    fn an_absent_supply_carries_neither_a_state_nor_system_attributes() {
        assert!(SnapshotSupply::Absent.materialize().is_none());
        assert!(SnapshotSupply::Absent.sys_attrs().is_none());
    }

    /// Clause 5.8.6: a match carries the state shared as it is and the clause
    /// 4.8 system attributes of that state beside it.
    #[test]
    fn a_carried_state_is_shared_and_carries_its_system_attributes_beside_it() {
        let state = building();
        let sys_attrs = sys_attrs();
        let supply = SnapshotSupply::Carried {
            state: &state,
            sys_attrs: Some(&sys_attrs),
        };
        let materialized = supply.materialize().expect("a carried state");
        assert!(Arc::ptr_eq(&materialized, &state));
        pretty_assertions::assert_eq!(supply.sys_attrs().as_ref(), Some(&sys_attrs));
    }
}
