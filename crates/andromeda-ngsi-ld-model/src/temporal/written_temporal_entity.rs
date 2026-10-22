//! The Temporal Evolution a client writes (ETSI GS CIM 009 clauses 5.6.11 and
//! 5.6.12), decoded from the text of its body.
//!
//! Two representations may carry the instances (clauses 4.5.7 and 4.5.9): the
//! normalized one, an array of instances per attribute, and the simplified
//! one, an object per `datasetId` holding `[value, instant]` pairs, which is
//! unfolded into the instances the pairs stand for as the body is read
//! ([`crate::temporal::simplified_unfold`]). The Scope is either the paths it
//! names or, being "a temporal representation of a Property" (clause 4.5.6), a
//! history of instances dating the paths; the two are told apart where the
//! body is read, since a reified Scope is recorded beside the entity and the
//! paths are carried by it.
//!
//! A fragment names neither `id` nor `type` where the request path names the
//! evolution (clause 5.6.12.3), so both are optional here and the operation
//! decides what it requires.

use crate::{
    attribute::{
        instance::{Attribute, AttributeValue},
        instance_member::ReadError,
        key_screening::{member_segment, screen_keys_in_instances},
        member_decoding::refusal,
    },
    attribute_strip::AttributeStrip,
    compact_member::decoded_optional,
    entity::EntityPatch,
    entity_type::EntityTypeValue,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
    request_body::RequestBody,
    scope_member::ScopeMember,
    temporal::simplified_unfold::instances_of,
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    time::{date_time::NgsiLdDateTime, rendering::instant_text},
};
use andromeda_ngsi_ld_representation::{
    compact_key_admission::CompactKeyError,
    compact_node::{CompactNode, NodeShape, produced_node::ProducedNode},
    embedded_context_admission::is_native_json_member,
};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned, ser::SerializeMap};
use serde_json::value::RawValue;
use std::str::FromStr;

/// The Scope of a written Temporal Evolution.
#[derive(Clone, Debug)]
pub enum WrittenScope {
    /// The Scope paths the entity is in, as clause 4.18 writes them.
    Paths(ScopeMember),
    /// The instances of a reified Scope, each dating the paths it names
    /// (clause 4.5.6).
    Reified(Vec<Attribute>),
}

/// A Temporal Evolution a client wrote.
#[derive(Clone, Debug)]
pub struct WrittenTemporalEntity {
    /// The identifier the body names, when it names one.
    pub id: Option<Identifier>,
    /// The types the body names, when it names any.
    pub type_: Option<EntityTypeValue>,
    /// The Scope the body writes.
    pub scope: Option<WrittenScope>,
    /// The entity expiry the body writes (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The instances each attribute adds, in order, simplified attributes
    /// unfolded.
    pub attrs: IndexMap<AttributeName, Vec<Attribute>>,
    /// The attributes the body wrote as one object rather than as an array of
    /// instances. Reading treats the two alike, as JSON-LD expansion does; only
    /// the Fragment of clause 5.6.14.3, which is "contained by an Array of
    /// exactly one item", tells them apart.
    pub unarrayed: Vec<AttributeName>,
}

impl WrittenTemporalEntity {
    /// The Temporal Evolution a compact node is.
    ///
    /// The entity's own system times, which a Context Producer may not supply
    /// (clause 5.2.2), are not read.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node that is no object, a
    /// member that does not decode, an attribute named by no attribute name,
    /// an instance no clause admits or an attribute object of the simplified
    /// representation clause 4.5.9 gives no reading of, each located at the
    /// member, and [`NodeReadError::Unreadable`] when a node of it has no JSON
    /// form.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<WrittenTemporalEntity, ReadError<'n, N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        let mut written = WrittenTemporalEntity {
            id: None,
            type_: None,
            scope: None,
            expires_at: None,
            attrs: IndexMap::new(),
            unarrayed: Vec::new(),
        };
        for (name, member) in members {
            let at = |error: ReadError<'n, N>| error.within_member(&name);
            match name.as_ref() {
                keyword::CONTEXT | compact_properties::CREATED_AT | compact_properties::MODIFIED_AT | compact_properties::DELETED_AT => {}
                keyword::ID_ALIAS => written.id = optional(member, RefusedMember::Id).map_err(at)?,
                keyword::TYPE_ALIAS => written.type_ = optional(member, RefusedMember::Type).map_err(at)?,
                compact_properties::EXPIRES_AT => written.expires_at = optional(member, RefusedMember::ExpiresAt).map_err(at)?.map(NgsiLdDateTime::get),
                compact_properties::SCOPE => written.scope = written_scope(member).map_err(at)?,
                attribute => {
                    let attribute_name = AttributeName::from_str(attribute).map_err(|source| at(NodeRefusal::AttributeName(source).into()))?;
                    let instances = instances_of(attribute, member).map_err(at)?;
                    if !matches!(member.shape(), NodeShape::Array(_)) && !written.unarrayed.contains(&attribute_name) {
                        written.unarrayed.push(attribute_name.clone()); // clone: the list names the attribute the map is keyed by
                    }
                    written.attrs.shift_remove(&attribute_name);
                    written.attrs.insert(attribute_name, instances);
                }
            }
        }
        Ok(written)
    }

    /// Whether the body writes any attribute (clause 5.6.11.4: "the
    /// remaining input data").
    #[must_use]
    pub fn has_attributes(&self) -> bool {
        !self.attrs.is_empty()
    }

    /// Take the named attributes out of the body, reporting what is left.
    pub fn strip_attributes(&mut self, attributes: &[AttributeName]) -> AttributeStrip {
        let before = self.attrs.len();
        for attribute in attributes {
            self.attrs.shift_remove(attribute.as_str());
        }
        self.unarrayed.retain(|written| !attributes.contains(written));
        AttributeStrip::from_counts(before, self.attrs.len())
    }

    /// Screen the body for the keys clause 4.6.4 forbids: the ones JSON-LD
    /// expansion would drop or rewrite past recovery, in the order the compact
    /// JSON states them. Read off the decoded instances, never off a copy of
    /// the whole body.
    ///
    /// # Errors
    ///
    /// Returns the first key clause 4.6.4 refuses.
    pub fn screen_keys(&self) -> Result<(), CompactKeyError> {
        let mut members: Vec<(&str, &[Attribute])> = self.attrs.iter().map(|(name, instances)| (name.as_str(), instances.as_slice())).collect();
        if let Some(WrittenScope::Reified(instances)) = &self.scope {
            members.push((compact_properties::SCOPE, instances));
        }
        // The rule walks a JSON object, whose members come in name order.
        members.sort_unstable_by_key(|(name, _)| *name);
        members
            .into_iter()
            .filter(|(name, _)| !is_native_json_member(name))
            .try_for_each(|(name, instances)| screen_keys_in_instances(instances, &[member_segment(name)]))
    }

    /// The body as the entity fragment its paths and instances are, and the
    /// reified Scope it dates, if any.
    #[must_use]
    pub fn into_fragment(self) -> (EntityPatch<AttributeValue>, Option<Vec<Attribute>>) {
        let (scope, reified) = match self.scope {
            Some(WrittenScope::Paths(paths)) => (Some(paths), None),
            Some(WrittenScope::Reified(instances)) => (None, Some(instances)),
            None => (None, None),
        };
        let fragment = EntityPatch {
            id: self.id,
            type_: self.type_,
            scope,
            expires_at: self.expires_at,
            location: None,
            observation_space: None,
            operation_space: None,
            attrs: self
                .attrs
                .into_iter()
                .map(|(name, instances)| (name, AttributeValue::Multiple(instances)))
                .collect(),
        };
        (fragment, reified)
    }
}

/// The body as a client would write it normalized: what a Context Source is
/// forwarded.
// Hand-written: the Scope is either a paths member or an instance array, and
// every attribute is the array of its instances; no derive writes either.
impl Serialize for WrittenTemporalEntity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(None)?;
        if let Some(id) = &self.id {
            object.serialize_entry(keyword::ID_ALIAS, id)?;
        }
        if let Some(type_) = &self.type_ {
            object.serialize_entry(keyword::TYPE_ALIAS, type_)?;
        }
        match &self.scope {
            Some(WrittenScope::Paths(paths)) => object.serialize_entry(compact_properties::SCOPE, paths)?,
            Some(WrittenScope::Reified(instances)) => object.serialize_entry(compact_properties::SCOPE, instances)?,
            None => {}
        }
        if let Some(expires_at) = &self.expires_at {
            object.serialize_entry(compact_properties::EXPIRES_AT, &instant_text(*expires_at))?;
        }
        for (name, instances) in &self.attrs {
            object.serialize_entry(name.as_str(), instances)?;
        }
        object.end()
    }
}

// Hand-written: the body is read by the compact model's reading of a node,
// which no derive expresses.
impl<'de> Deserialize<'de> for WrittenTemporalEntity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WrittenTemporalEntity, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        WrittenTemporalEntity::from_node(literal.piece()).map_err(refusal)
    }
}

impl RequestBody for WrittenTemporalEntity {
    /// Decode a Temporal Evolution from the JSON text of a request body, read
    /// in place as a Context Producer wrote it.
    fn from_request_text(text: &[u8]) -> Result<WrittenTemporalEntity, serde_json::Error> {
        let raw: &RawValue = serde_json::from_slice(text)?;
        WrittenTemporalEntity::from_node(ProducedNode::recording_deletions(JsonPiece::new(raw))).map_err(refusal)
    }
}

/// Read a member that may be absent or `null`.
fn optional<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<Option<T>, ReadError<'n, N>> {
    decoded_optional(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// The Scope a node writes: a history of instances when it is one object or an
/// array of objects (clause 4.5.6), the paths of clause 4.18 otherwise.
fn written_scope<'n, N: CompactNode<'n>>(node: N) -> Result<Option<WrittenScope>, ReadError<'n, N>> {
    let reified = match node.shape() {
        NodeShape::Object(_) => true,
        NodeShape::Array(items) => !items.is_empty() && items.iter().all(|item| matches!(item.shape(), NodeShape::Object(_))),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Geometry(_) => false,
    };
    if reified {
        let instances = instances_of(compact_properties::SCOPE, node)?;
        return Ok(Some(WrittenScope::Reified(instances)));
    }
    let paths: Option<ScopeMember> = optional(node, RefusedMember::Scope)?;
    Ok(paths.map(WrittenScope::Paths))
}

#[cfg(test)]
mod tests;
