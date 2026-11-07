//! Splitting one stored instance identity across its two history columns.
//!
//! Attribute history and scope history hold an instance identifier the same
//! way — a broker-minted UUID in one column, a verbatim identifier in the other,
//! exactly one of them populated — so the split is written once here rather than
//! restated per table. [`InstanceIdentity`] is owned by the lexicon and cannot
//! carry these accessors itself, since they are about columns rather than about
//! what an identifier is.

use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
use uuid::Uuid;

/// The broker-minted identifier column's content, when the row uses it.
#[must_use]
pub const fn generated_instance_id(instance: &InstanceIdentity) -> Option<Uuid> {
    match instance {
        InstanceIdentity::Generated(uuid) => Some(*uuid),
        InstanceIdentity::Verbatim(_) => None,
    }
}

/// The verbatim identifier column's content, when the row uses it.
///
/// A populated verbatim column is also what marks the instance as one the
/// broker did not mint and must hand back unchanged (clause 4.5.7).
#[must_use]
pub fn verbatim_instance_id(instance: &InstanceIdentity) -> Option<&str> {
    match instance {
        InstanceIdentity::Generated(_) => None,
        InstanceIdentity::Verbatim(identifier) => Some(identifier.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use crate::stored_instance_columns::{generated_instance_id, verbatim_instance_id};
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use uuid::Uuid;

    #[test]
    fn a_broker_minted_identity_populates_only_the_generated_column() {
        let identity = InstanceIdentity::Generated(Uuid::from_u128(7));
        pretty_assertions::assert_eq!(generated_instance_id(&identity), Some(Uuid::from_u128(7)));
        assert!(verbatim_instance_id(&identity).is_none());
    }

    #[test]
    fn a_client_supplied_identity_populates_only_the_verbatim_column() {
        let identity = InstanceIdentity::Verbatim("https://example.com/instances/1".parse().expect("a valid URI"));
        assert!(generated_instance_id(&identity).is_none());
        pretty_assertions::assert_eq!(verbatim_instance_id(&identity), Some("https://example.com/instances/1"));
    }
}
