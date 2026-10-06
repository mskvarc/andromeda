//! The members every NGSI-LD attribute family carries (ETSI GS CIM 009 clause 5.2).

use crate::attribute::{instance::AttributeValue, nested_null::NestedMember};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Members present on all NGSI-LD attribute families (ETSI GS CIM 009 clause 5.2).
///
/// Every member is written flat beside the family's own, the sub-attributes
/// among them.
#[derive(Debug, Clone, Default)]
pub struct CommonAttributeFields {
    /// System-generated creation timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Dataset identifier separating multi-instance attributes.
    pub dataset_id: Option<Identifier>,
    /// System-generated deletion timestamp.
    pub deleted_at: Option<DateTime<Utc>>,
    /// Attribute expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// URI uniquely identifying an attribute instance.
    pub instance_id: Option<Identifier>,
    /// System-generated last modification timestamp.
    pub modified_at: Option<DateTime<Utc>>,
    /// Observation timestamp for the attribute value.
    pub observed_at: Option<DateTime<Utc>>,
    /// Attributes of this attribute, keyed by attribute name.
    pub sub_attributes: IndexMap<AttributeName, AttributeValue>,
}

// Hand-written: the members are written flat beside the family's own, and a
// sub-attribute is written as a member of the instance, where the NGSI-LD Null
// is the value of a concise Property rather than the deletion of the member
// (clause 4.5.2.3); a derive writes the sub-attributes through the member's own
// serialization, which cannot know its depth.
impl Serialize for CommonAttributeFields {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        optional_entry(&mut map, compact_properties::CREATED_AT, self.created_at.as_ref())?;
        optional_entry(&mut map, compact_properties::DATASET_ID, self.dataset_id.as_ref())?;
        optional_entry(&mut map, compact_properties::DELETED_AT, self.deleted_at.as_ref())?;
        optional_entry(&mut map, compact_properties::EXPIRES_AT, self.expires_at.as_ref())?;
        optional_entry(&mut map, compact_properties::INSTANCE_ID, self.instance_id.as_ref())?;
        optional_entry(&mut map, compact_properties::MODIFIED_AT, self.modified_at.as_ref())?;
        optional_entry(&mut map, compact_properties::OBSERVED_AT, self.observed_at.as_ref())?;
        for (name, member) in &self.sub_attributes {
            map.serialize_entry(name.as_str(), &NestedMember(member))?;
        }
        map.end()
    }
}

/// Write `value` under `key` when present.
fn optional_entry<M: SerializeMap, T: Serialize>(map: &mut M, key: &str, value: Option<&T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| map.serialize_entry(key, value))
}
