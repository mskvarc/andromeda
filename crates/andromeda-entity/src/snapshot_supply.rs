//! The entity state a matched subscription is filtered and notified with.
//!
//! A creation or an update hands its subscriptions the state the write left
//! behind, shared as it is. A deletion hands them the state it removed, which
//! the durable match stores with its clause 4.8 system attributes embedded:
//! by delivery time the row is gone and `createdAt` and `modifiedAt` can no
//! longer be read back (clause 5.8.6). Building that enriched copy is the one
//! expensive step, so it is deferred until a subscription has actually matched
//! and then done once, every match sharing the result.

use crate::sys_attrs::snapshot_with_sys_attrs;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use serde_json::Value;
use std::sync::Arc;

/// The entity state a matched subscription is filtered and notified with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SnapshotSupply<'a> {
    /// The mutation carries no rendered state.
    Absent,
    /// The state the mutation left behind, shared as it is.
    Ready(&'a Arc<Value>),
    /// A deletion's removed state, enriched with its clause 4.8 system
    /// attributes the first time a match needs it.
    Removed {
        /// The state the deletion removed.
        removed: &'a Arc<Value>,
        /// The system attributes captured before the deletion, when they could
        /// be read.
        sys_attrs: Option<&'a SysAttrsSnapshot>,
    },
}

impl SnapshotSupply<'_> {
    /// The state a matched subscription is notified with: the shared state
    /// itself, or for a deletion with system attributes, the one enriched copy
    /// built for every match of the change.
    pub(crate) fn materialize(self) -> Option<Arc<Value>> {
        match self {
            SnapshotSupply::Absent => None,
            SnapshotSupply::Ready(state)
            | SnapshotSupply::Removed {
                removed: state,
                sys_attrs: None,
            } => Some(Arc::clone(state)),
            SnapshotSupply::Removed {
                removed,
                sys_attrs: Some(sys_attrs),
            } => Some(Arc::new(snapshot_with_sys_attrs(removed, sys_attrs))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{snapshot_supply::SnapshotSupply, sys_attrs::snapshot_with_sys_attrs};
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// A removed entity carrying one attribute instance.
    fn removed() -> Arc<Value> {
        Arc::new(json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            "https://uri.etsi.org/ngsi-ld/default-context/category": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}]
            }]
        }]))
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
        let materialized = SnapshotSupply::Removed {
            removed: &removed,
            sys_attrs: None,
        }
        .materialize()
        .expect("a removed state");
        assert!(Arc::ptr_eq(&materialized, &removed));
    }

    #[test]
    fn a_ready_snapshot_is_the_shared_one() {
        let current = removed();
        let materialized = SnapshotSupply::Ready(&current).materialize().expect("a current state");
        assert!(Arc::ptr_eq(&materialized, &current));
        assert!(SnapshotSupply::Absent.materialize().is_none());
    }

    /// Clause 5.8.6: the durable match of a deletion stores the removed state
    /// with its clause 4.8 system attributes, as `snapshot_with_sys_attrs`
    /// builds it.
    #[test]
    fn a_removed_snapshot_is_enriched_with_its_system_attributes_once() {
        let removed = removed();
        let sys_attrs = sys_attrs();
        let materialized = SnapshotSupply::Removed {
            removed: &removed,
            sys_attrs: Some(&sys_attrs),
        }
        .materialize()
        .expect("a removed state");
        assert!(!Arc::ptr_eq(&materialized, &removed));
        pretty_assertions::assert_eq!(*materialized, snapshot_with_sys_attrs(&removed, &sys_attrs));
        assert!(materialized.to_string().contains("createdAt"), "{materialized}");
    }
}
