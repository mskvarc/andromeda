//! A member of a merge fragment that either sets a value or removes the one
//! held (ETSI GS CIM 009 clause 5.5.12).
//!
//! Clause 5.5.12 merges a fragment "up to an arbitrary depth", and "NGSI-LD
//! Nulls within the Fragment [...] indicate the removal of existing values
//! within the target member value". A member of a merge fragment is therefore
//! one of two things: the value it sets, or the NGSI-LD Null that removes the
//! value held.

use crate::{
    attribute::instance_member::{ReadError, optional_member},
    node_refusal::RefusedMember,
};
use andromeda_ngsi_ld_lexicon::rfc3339::RequestInstant;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, de::DeserializeOwned};

/// A merge fragment member: a value to set, or the removal of the one held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Patched<T> {
    /// The value the member sets.
    Set(T),
    /// The NGSI-LD Null: the value held is removed.
    Null,
}

// Hand-written: the removal is written as the NGSI-LD Null string, which no
// derive of an enum expresses beside the untagged value.
impl<T: Serialize> Serialize for Patched<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Patched::Set(value) => value.serialize(serializer),
            Patched::Null => serializer.serialize_str(NULL_URI.as_str()),
        }
    }
}

/// Whether `node` is the NGSI-LD Null written as a JSON string.
pub(crate) fn is_ngsi_ld_null<'n, N: CompactNode<'n>>(node: N) -> bool {
    match node.shape() {
        NodeShape::String(text) => text == NULL_URI.as_str(),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => false,
    }
}

/// Read an optional member of a merge fragment: absent and JSON `null` are no
/// member, the NGSI-LD Null is a removal, anything else the `T` it decodes as.
///
/// # Errors
///
/// Returns the refusal of a member that does not decode as a `T`, located at
/// the member, and the node's [`CompactNode::Unreadable`].
pub(crate) fn patched_member<'n, T: DeserializeOwned, N: CompactNode<'n>>(
    member: Option<N>,
    name: RefusedMember,
) -> Result<Option<Patched<T>>, ReadError<'n, N>> {
    match member {
        Some(node) if is_ngsi_ld_null(node) => Ok(Some(Patched::Null)),
        Some(_) | None => Ok(optional_member(member, name)?.map(Patched::Set)),
    }
}

/// Read a merge fragment instant member (`observedAt`, `expiresAt`),
/// accepting the clause 4.6.3 comma separator.
///
/// # Errors
///
/// Returns the refusal of a member that is not an RFC 3339 date-time, located
/// at the member, and the node's [`CompactNode::Unreadable`].
pub(crate) fn patched_instant_member<'n, N: CompactNode<'n>>(
    member: Option<N>,
    name: RefusedMember,
) -> Result<Option<Patched<DateTime<Utc>>>, ReadError<'n, N>> {
    Ok(patched_member::<RequestInstant, N>(member, name)?.map(|patched| match patched {
        Patched::Set(instant) => Patched::Set(instant.get()),
        Patched::Null => Patched::Null,
    }))
}

#[cfg(test)]
mod tests {
    use crate::attribute::patched::Patched;

    #[test]
    fn a_removal_is_written_as_the_ngsi_ld_null() {
        pretty_assertions::assert_eq!(serde_json::to_string(&Patched::<u8>::Null).expect("written"), r#""urn:ngsi-ld:null""#);
        pretty_assertions::assert_eq!(serde_json::to_string(&Patched::Set(7_u8)).expect("written"), "7");
    }
}
