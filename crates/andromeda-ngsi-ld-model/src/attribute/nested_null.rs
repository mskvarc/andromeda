//! A sub-attribute as the compact serialization of its instance writes it.
//!
//! The NGSI-LD Null written in place of a whole attribute member is the
//! deletion of the member where a write or a notification says so (clauses
//! 5.5.12 and 5.8.6), and only there: below the top level, written as the
//! value of a member of an instance, it is the value of a concise Property
//! (clause 4.5.2.3). A sub-attribute is therefore written with that reading
//! made, so a reader of the instance's JSON -- the JSON-LD processor above all
//! -- never takes the Null for a string value of a term.

use crate::attribute::{instance::AttributeValue, merge_member::MergeMember};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// The member an instance names its family in.
const TYPE_MEMBER: &str = "type";

/// The member a Property holds its value in.
const VALUE_MEMBER: &str = "value";

/// The Property the NGSI-LD Null written where an attribute stands, and not
/// as its deletion, stands for (clause 4.5.2.3).
#[derive(Clone, Copy, Debug)]
pub struct NullProperty;

// Hand-written: a constant object of two members, which no derive writes.
impl Serialize for NullProperty {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(TYPE_MEMBER, "Property")?;
        object.serialize_entry(VALUE_MEMBER, NULL_URI.as_str())?;
        object.end()
    }
}

/// A sub-attribute of an entity's attribute instance.
pub(crate) struct NestedMember<'a>(pub(crate) &'a AttributeValue);

impl Serialize for NestedMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            AttributeValue::NgsiLdNull => NullProperty.serialize(serializer),
            AttributeValue::Single(_) | AttributeValue::Multiple(_) => self.0.serialize(serializer),
        }
    }
}

/// A sub-attribute of a merge fragment's attribute instance.
pub(crate) struct NestedMergeMember<'a>(pub(crate) &'a MergeMember);

impl Serialize for NestedMergeMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            MergeMember::NgsiLdNull => NullProperty.serialize(serializer),
            MergeMember::Single(_) | MergeMember::Multiple(_) => self.0.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use serde_json::{Value, json};

    /// Clause 4.5.2.3: a Null written as a sub-attribute is the value of a
    /// concise Property, and is written as that Property.
    #[test]
    fn a_nested_null_is_written_as_the_property_it_stands_for() {
        let instance: Attribute = serde_json::from_value(json!({"type": "Property", "value": 1, "unit": "urn:ngsi-ld:null"})).expect("an instance");
        let written: Value = serde_json::to_value(&instance).expect("the instance writes");
        pretty_assertions::assert_eq!(written["unit"], json!({"type": "Property", "value": "urn:ngsi-ld:null"}));
    }
}
