//! The body of an entity write forwarded to a context source, written from its
//! typed form (clauses 4.3.6.6, 4.3.6.8, 5.6.1 to 5.6.3, 5.6.11, 5.6.17 and
//! 5.6.18).
//!
//! A forwarded write names its entity members as the compact partial entity
//! the request's admitted content compacts to under the registration's
//! forwarding context, and every attribute is written in the normalized
//! representation of clause 4.5.1: the one representation every version of the
//! specification reads. The registration's `ngsildConformance` is the clause
//! 4.3.6.8 fallback the typed walk applies as it writes, so no body is first
//! serialised and then rewritten. Tables 4.3.6.8-1 to -3 describe Entity data
//! alone: a Subscription or a Query is never written here.
//!
//! The members the patch leaves out are left out: a fragment names neither its
//! `id` nor its `type` where the request path names the entity.

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        notified_member::MemberInstances,
        reified_member::{Reification, ReifiedMember},
    },
    version::{
        adapted_payload::AdaptedPayload,
        fallback::{EntityMember, VersionFallback},
    },
};
use andromeda_ngsi_ld_lexicon::{spec_version::SpecVersion, time::rendering::instant_text};
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::EntityPatch};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::slice::from_ref;

/// What an attribute's JSON array holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceLayout {
    /// The clause 4.5.5 set of instances, one per `datasetId`.
    Datasets,
    /// The clause 4.5.7 sequence of instances in time, which is an array even
    /// for one instance.
    Sequence,
}

impl InstanceLayout {
    /// The layout the Element a payload is has.
    const fn of(payload: AdaptedPayload) -> InstanceLayout {
        match payload {
            AdaptedPayload::TemporalEvolution => InstanceLayout::Sequence,
            AdaptedPayload::Entity | AdaptedPayload::SimplifiedTemporalEvolution => InstanceLayout::Datasets,
        }
    }
}

/// One entity, or entity fragment, as a forwarded write carries it.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedEntity<'a> {
    /// The compact partial entity written.
    patch: &'a EntityPatch,
    /// What every member is written under.
    walk: MemberWalk<'a>,
    /// What an attribute's array holds.
    layout: InstanceLayout,
    /// The reified Scope of a Temporal Evolution (clause 4.5.6), written as
    /// the member `scope` in place of the Scope paths the patch carries.
    scope_history: Option<&'a AttributeValue>,
}

impl<'a> ForwardedEntity<'a> {
    /// `patch`, an `element` payload, written for a source that conforms to
    /// `conformance`, when the registration names a version.
    #[must_use]
    pub const fn new(patch: &'a EntityPatch, conformance: Option<SpecVersion>, element: AdaptedPayload) -> ForwardedEntity<'a> {
        let fallback = match conformance {
            Some(version) => VersionFallback::new(version, element),
            None => None,
        };
        ForwardedEntity {
            patch,
            walk: MemberWalk::new(None, fallback),
            layout: InstanceLayout::of(element),
            scope_history: None,
        }
    }

    /// The entity written with `scope_history`, the instances of a reified
    /// Scope (clause 4.5.6), as its `scope` member. A Temporal Evolution
    /// names the Scope either as paths or as this history, never both.
    #[must_use]
    pub const fn with_scope_history(mut self, scope_history: &'a AttributeValue) -> ForwardedEntity<'a> {
        self.scope_history = Some(scope_history);
        self
    }
}

impl Serialize for ForwardedEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let patch = self.patch;
        let mut object = serializer.serialize_map(None)?;
        if let Some(id) = &patch.id {
            object.serialize_entry(keyword::ID_ALIAS, id)?;
        }
        if let Some(types) = &patch.type_ {
            match types.iter().next() {
                Some(first) if self.walk.writes_single_type() => object.serialize_entry(keyword::TYPE_ALIAS, first)?,
                Some(_) | None => object.serialize_entry(keyword::TYPE_ALIAS, types)?,
            }
        }
        if let Some(scope) = &patch.scope
            && self.walk.keeps_entity_member(EntityMember::Scope)
        {
            object.serialize_entry(compact_properties::SCOPE, scope)?;
        }
        if let Some(history) = self.scope_history
            && self.walk.keeps_entity_member(EntityMember::Scope)
        {
            object.serialize_entry(compact_properties::SCOPE, &ForwardedAttribute::member(history, self.walk, self.layout))?;
        }
        if let Some(expires_at) = &patch.expires_at
            && self.walk.keeps_entity_member(EntityMember::ExpiresAt)
        {
            object.serialize_entry(compact_properties::EXPIRES_AT, &instant_text(*expires_at))?;
        }
        for (name, member) in patch.attribute_members() {
            object.serialize_entry(name, &ForwardedAttribute::member(member, self.walk, self.layout))?;
        }
        object.end()
    }
}

/// The one attribute a partial attribute update or a replacement carries
/// (clauses 5.6.4 and 5.6.19): the instance alone, without its name.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedAttribute<'a> {
    /// The compact attribute written.
    member: &'a AttributeValue,
    /// What the member is written under.
    walk: MemberWalk<'a>,
    /// What the attribute's array holds.
    layout: InstanceLayout,
}

impl<'a> ForwardedAttribute<'a> {
    /// `member`, an `element` payload's attribute, written for a source that
    /// conforms to `conformance`, when the registration names a version.
    #[must_use]
    pub const fn new(member: &'a AttributeValue, conformance: Option<SpecVersion>, element: AdaptedPayload) -> ForwardedAttribute<'a> {
        let fallback = match conformance {
            Some(version) => VersionFallback::new(version, element),
            None => None,
        };
        ForwardedAttribute::member(member, MemberWalk::new(None, fallback), InstanceLayout::of(element))
    }

    /// `member` written under `walk` and `layout`.
    const fn member(member: &'a AttributeValue, walk: MemberWalk<'a>, layout: InstanceLayout) -> ForwardedAttribute<'a> {
        ForwardedAttribute { member, walk, layout }
    }

    /// The instances the member is written as.
    fn instances(&self) -> MemberInstances<'a> {
        match (self.layout, self.member) {
            (InstanceLayout::Sequence, AttributeValue::Single(attribute)) => MemberInstances::Evolution(from_ref(attribute)),
            (InstanceLayout::Sequence, AttributeValue::Multiple(instances)) => MemberInstances::Evolution(instances),
            (InstanceLayout::Datasets, held @ (AttributeValue::Single(_) | AttributeValue::Multiple(_)))
            | (InstanceLayout::Sequence | InstanceLayout::Datasets, held @ AttributeValue::NgsiLdNull) => MemberInstances::Held(held),
        }
    }
}

impl Serialize for ForwardedAttribute<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReifiedMember::new(self.instances(), Reification::Normalized, self.walk).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        forwarded_entity::{ForwardedAttribute, ForwardedEntity},
        version::adapted_payload::AdaptedPayload,
    };
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::EntityPatch};
    use serde_json::{Value, json};

    fn patch(written: &Value) -> EntityPatch {
        serde_json::from_value(written.clone()).expect("a partial entity") // clone: the fixture keeps its written form
    }

    fn written(entity: &EntityPatch, conformance: Option<SpecVersion>, element: AdaptedPayload) -> Value {
        serde_json::to_value(ForwardedEntity::new(entity, conformance, element)).expect("the entity writes")
    }

    /// An entity is written normalized, whatever representation it was read in.
    #[test]
    fn a_concise_entity_is_written_normalized() {
        let entity = patch(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": 10}));
        pretty_assertions::assert_eq!(
            written(&entity, None, AdaptedPayload::Entity),
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}})
        );
    }

    /// Clause 5.6.2.3: a fragment names neither its `id` nor its `type`.
    #[test]
    fn a_fragment_writes_only_the_members_it_names() {
        let entity = patch(&json!({"speed": {"type": "Property", "value": 10}}));
        pretty_assertions::assert_eq!(
            written(&entity, None, AdaptedPayload::Entity),
            json!({"speed": {"type": "Property", "value": 10}})
        );
    }

    /// Table 4.3.6.8-1: a source conforming to 1.0 reads one `type`, no
    /// `scope`, and no instance member introduced after it.
    #[test]
    fn the_fallback_of_a_1_0_source_is_applied_as_the_entity_is_written() {
        let entity = patch(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": ["Vehicle", "Car"],
            "scope": "/Madrid",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "bus"}},
            "speed": {"type": "Property", "value": 10, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "KMH"}
        }));
        let version: SpecVersion = "1.0".parse().expect("a version");
        pretty_assertions::assert_eq!(
            written(&entity, Some(version), AdaptedPayload::Entity),
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "name": {"type": "Property", "value": {"en": "bus"}},
                "speed": {"type": "Property", "value": 10}
            })
        );
    }

    /// Clause 4.5.7: the array of a temporal body is the sequence of instances
    /// in time, so a single instance is still an array and `observedAt` stays.
    #[test]
    fn a_temporal_body_keeps_its_instance_sequence_under_the_fallback() {
        let entity = patch(&json!({
            "speed": [
                {"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"},
                {"type": "Property", "value": 2, "observedAt": "2025-01-02T00:00:00Z"}
            ],
            "rpm": {"type": "Property", "value": 3, "observedAt": "2025-01-01T00:00:00Z"}
        }));
        let version: SpecVersion = "1.0".parse().expect("a version");
        let body = written(&entity, Some(version), AdaptedPayload::TemporalEvolution);
        pretty_assertions::assert_eq!(body["speed"].as_array().map(Vec::len), Some(2));
        assert!(body["speed"][0].get("observedAt").is_some(), "{body}");
        pretty_assertions::assert_eq!(body["rpm"].as_array().map(Vec::len), Some(1));
    }

    /// Clause 4.5.6: a reified Scope is written as the sequence of its
    /// instances, each with its temporal sub-properties, and a 1.0 source,
    /// which reads no `scope`, is sent none.
    #[test]
    fn a_reified_scope_is_written_as_its_instance_sequence() {
        let entity = patch(&json!({"speed": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}]}));
        let history: AttributeValue = serde_json::from_value(json!([
            {"type": "Property", "value": "/Madrid", "observedAt": "2025-01-01T00:00:00Z", "createdAt": "2025-01-01T00:00:01Z"},
            {"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2025-01-02T00:00:00Z", "deletedAt": "2025-01-02T00:00:00Z"}
        ]))
        .expect("a reified scope");
        let body = serde_json::to_value(ForwardedEntity::new(&entity, None, AdaptedPayload::TemporalEvolution).with_scope_history(&history))
            .expect("the entity writes");
        let scope = body["scope"].as_array().expect("the scope is a sequence");
        pretty_assertions::assert_eq!(scope.len(), 2);
        pretty_assertions::assert_eq!(scope[0]["value"], json!("/Madrid"));
        pretty_assertions::assert_eq!(scope[0]["createdAt"], json!("2025-01-01T00:00:01Z"));
        pretty_assertions::assert_eq!(scope[1]["deletedAt"], json!("2025-01-02T00:00:00Z"));
        let version: SpecVersion = "1.0".parse().expect("a version");
        let old = serde_json::to_value(ForwardedEntity::new(&entity, Some(version), AdaptedPayload::TemporalEvolution).with_scope_history(&history))
            .expect("the entity writes");
        assert!(old.get("scope").is_none(), "{old}");
    }

    /// Tables 4.3.6.8-2 and -3 reach the instance of the one attribute a
    /// request path names.
    #[test]
    fn an_attribute_is_written_as_its_instance_under_the_fallback() {
        let attribute: AttributeValue = serde_json::from_value(json!({
            "type": "Property",
            "value": 10,
            "observedAt": "2025-01-01T00:00:00Z",
            "unitCode": "KMH",
            "valueType": "Speed"
        }))
        .expect("an attribute");
        let version: SpecVersion = "1.0".parse().expect("a version");
        let body = serde_json::to_value(ForwardedAttribute::new(&attribute, Some(version), AdaptedPayload::Entity)).expect("the attribute writes");
        pretty_assertions::assert_eq!(body, json!({"type": "Property", "value": 10}));
    }
}
