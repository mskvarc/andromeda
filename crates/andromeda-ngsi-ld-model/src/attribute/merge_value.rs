//! The value member of an attribute instance in a merge fragment (ETSI GS CIM
//! 009 clause 5.5.12), in the shape the instance's family holds it.

use crate::{
    attribute::{
        geo_property_value::GeoPropertyValue,
        instance_node::{geo_value, json, member, ngsi_ld_value, value_list},
        language_property::{LanguageMap, LanguageMapValue},
        member_value_reading::ReadError,
        object_list::RelationshipObjectList,
        one_or_many::OneOrMany,
        value::NgsiLdValue,
    },
    node_refusal::RefusedMember,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, coerced_term::CoercedTerm, uri::Uri};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use serde::Serialize;

/// The value a merge fragment writes for an instance of one family.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MergeValue {
    /// A Property's `value`.
    Property(NgsiLdValue),
    /// A Relationship's `object`.
    Relationship(OneOrMany<Uri>),
    /// A `GeoProperty`'s `value`.
    GeoProperty(GeoPropertyValue),
    /// A `VocabProperty`'s `vocab`.
    VocabProperty(OneOrMany<CoercedTerm>),
    /// A `LanguageProperty`'s `languageMap`.
    LanguageProperty(LanguageMapPatch),
    /// A `JsonProperty`'s `json`.
    JsonProperty(JsonLiteral),
    /// A `ListProperty`'s `valueList`.
    ListProperty(Vec<NgsiLdValue>),
    /// A `ListRelationship`'s `objectList`.
    ListRelationship(RelationshipObjectList),
}

/// The `languageMap` a merge fragment writes.
///
/// Table 6.5.3.4-1's `lang` names the key "a string (or string array)" merged
/// into a `LanguageProperty` updates, so a merge fragment may write the strings
/// alone, untagged, for the merge to key by `lang`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum LanguageMapPatch {
    /// A language map, keyed by language tag.
    Map(LanguageMap),
    /// A string or an array of strings, keyed by the request's `lang`.
    Untagged(LanguageMapValue),
}

impl MergeValue {
    /// Read the value member of an instance of `family`.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a value that is none `family` holds, located at
    /// the member, and the node's [`CompactNode::Unreadable`].
    pub(crate) fn from_node<'n, N: CompactNode<'n>>(family: AttributeType, node: N) -> Result<MergeValue, ReadError<'n, N>> {
        let name = RefusedMember::Value(family);
        Ok(match family {
            AttributeType::Property => MergeValue::Property(ngsi_ld_value(node, name)?),
            AttributeType::Relationship => MergeValue::Relationship(member(node, name)?),
            AttributeType::GeoProperty => MergeValue::GeoProperty(geo_value(node, name)?),
            AttributeType::VocabProperty => MergeValue::VocabProperty(member(node, name)?),
            AttributeType::LanguageProperty => MergeValue::LanguageProperty(language_map(node, name)?),
            AttributeType::JsonProperty => MergeValue::JsonProperty(json(node, name)?),
            AttributeType::ListProperty => MergeValue::ListProperty(value_list(node, name)?),
            AttributeType::ListRelationship => MergeValue::ListRelationship(member(node, name)?),
        })
    }

    /// The family the value is one of.
    #[must_use]
    pub const fn family(&self) -> AttributeType {
        match self {
            MergeValue::Property(_) => AttributeType::Property,
            MergeValue::Relationship(_) => AttributeType::Relationship,
            MergeValue::GeoProperty(_) => AttributeType::GeoProperty,
            MergeValue::VocabProperty(_) => AttributeType::VocabProperty,
            MergeValue::LanguageProperty(_) => AttributeType::LanguageProperty,
            MergeValue::JsonProperty(_) => AttributeType::JsonProperty,
            MergeValue::ListProperty(_) => AttributeType::ListProperty,
            MergeValue::ListRelationship(_) => AttributeType::ListRelationship,
        }
    }
}

/// A `languageMap`: a language map, or the untagged strings `lang` keys.
fn language_map<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<LanguageMapPatch, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::String(_) | NodeShape::Array(_) => member(node, name).map(LanguageMapPatch::Untagged),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            member(node, name).map(LanguageMapPatch::Map)
        }
    }
}
