//! The NGSI-LD Entity and the partial form its patch operations accept (ETSI GS CIM 009 clause 5.2.4).

use crate::{
    attribute::instance::{Attribute, AttributeValue},
    entity_type::EntityTypeValue,
    notification::SysAttrsSnapshot,
    scope_member::ScopeMember,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::Serialize;
use serde_with::skip_serializing_none;
use typed_builder::TypedBuilder;

/// NGSI-LD Entity representation (ETSI GS CIM 009 clause 5.2.4, Table 5.2.4-1).
///
/// An Entity is the core informational unit in NGSI-LD, representing a
/// real-world object or concept. It has an identifier, one or more types,
/// optional scope, and a set of attributes (Properties, Relationships, etc.).
///
/// All spec-defined entity members are explicit fields. `@context` is excluded
/// as it is a transport concern handled by the JSON-LD engine. Decoding is
/// `crate::entity_decoding`'s, which reads the attributes without buffering
/// them.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Entity<M = AttributeValue> {
    /// Entity identifier (clause 5.2.4).
    pub id: Identifier,
    /// One or more entity types.
    #[serde(rename = "type")]
    pub type_: EntityTypeValue,
    /// Scopes the entity belongs to (clause 4.18).
    #[builder(default = Default::default())]
    pub scope: Option<ScopeMember>,

    /// Entity expiration timestamp (clause 4.22).
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,

    /// Default geospatial property (clause 4.7).
    #[builder(default = Default::default())]
    pub location: Option<M>,

    /// Observation space geometry (clause 5.2.4).
    #[builder(default = Default::default())]
    pub observation_space: Option<M>,

    /// Operation space geometry (clause 5.2.4).
    #[builder(default = Default::default())]
    pub operation_space: Option<M>,

    /// System-generated creation timestamp (clause 4.8, Table 5.2.2-1).
    #[builder(default = Default::default())]
    pub created_at: Option<DateTime<Utc>>,

    /// System-generated modification timestamp (clause 4.8, Table 5.2.2-1).
    #[builder(default = Default::default())]
    pub modified_at: Option<DateTime<Utc>>,

    /// Deletion timestamp, output-only in notifications/temporal (clause 4.8).
    #[builder(default = Default::default())]
    pub deleted_at: Option<DateTime<Utc>>,

    /// Every other attribute the entity carries, keyed by attribute name.
    #[serde(flatten)]
    #[builder(default = Default::default())]
    pub attrs: IndexMap<AttributeName, M>,
}

impl<M> Entity<M> {
    /// Populate system attribute fields from a repository snapshot.
    ///
    /// Sets `created_at` and `modified_at` unconditionally. Sets `expires_at`
    /// only if the field is not already populated (user-provided value takes
    /// precedence).
    #[must_use]
    pub const fn with_sys_attrs(mut self, snapshot: &SysAttrsSnapshot) -> Entity<M> {
        self.created_at = Some(snapshot.created_at);
        self.modified_at = Some(snapshot.modified_at);
        if self.expires_at.is_none() {
            self.expires_at = snapshot.expires_at;
        }
        self
    }

    /// The name of every Attribute member the entity carries, the three
    /// clause 5.2.4 geo-properties included.
    ///
    /// `location`, `observationSpace` and `operationSpace` are Attributes that
    /// this type keeps in a field of their own rather than in [`Entity::attrs`],
    /// because clause 4.7 and clause 5.2.4 give each a fixed meaning. Every
    /// narrowing that selects Attributes by name -- `attrs` (clause 5.7.1.3),
    /// `pick` and `omit` (clause 4.21) -- has to reach them all the same, so
    /// this is what such a narrowing iterates rather than `attrs` alone.
    pub fn attribute_member_names(&self) -> impl Iterator<Item = &str> {
        self.attribute_members().map(|(name, _value)| name)
    }

    /// Every Attribute member with its name, from wherever this type keeps it:
    /// [`Entity::attrs`] in order, then `location`, `observationSpace` and
    /// `operationSpace` where present.
    pub fn attribute_members(&self) -> impl Iterator<Item = (&str, &M)> {
        self.attrs.iter().map(|(name, value)| (name.as_str(), value)).chain(
            self.geo_property_members()
                .into_iter()
                .filter_map(|(name, value)| value.as_ref().map(|value| (name, value))),
        )
    }

    /// The Attribute member named `name`, from wherever this type keeps it.
    #[must_use]
    pub fn attribute_member(&self, name: &str) -> Option<&M> {
        match self.geo_property_members().into_iter().find(|(geo_name, _)| *geo_name == name) {
            Some((_, member)) => member.as_ref(),
            None => self.attrs.get(name),
        }
    }

    /// Keep only the Attribute members `keep` accepts, from wherever this type
    /// keeps them.
    ///
    /// `keep` sees exactly the members [`Entity::attribute_members`] yields,
    /// each mutably so that it can narrow the member it keeps, and the
    /// surviving members of [`Entity::attrs`] keep their order.
    pub fn retain_attribute_members(&mut self, mut keep: impl FnMut(&str, &mut M) -> bool) {
        self.attrs.retain(|name, member| keep(name.as_str(), member));
        for (name, slot) in [
            (compact_properties::LOCATION, &mut self.location),
            (compact_properties::OBSERVATION_SPACE, &mut self.observation_space),
            (compact_properties::OPERATION_SPACE, &mut self.operation_space),
        ] {
            if slot.as_mut().is_some_and(|member| !keep(name, member)) {
                *slot = None;
            }
        }
    }

    /// Whether the entity carries any Attribute member at all.
    ///
    /// Clause 5.7.1.4 turns an entity left with none by the requested
    /// Attribute list into a `ResourceNotFound`, so the question has to count
    /// the geo-properties too.
    #[must_use]
    pub fn has_attribute_members(&self) -> bool {
        !self.attrs.is_empty() || self.geo_property_members().iter().any(|(_, value)| value.is_some())
    }

    /// The three clause 5.2.4 geo-property members, each paired with the field
    /// this type keeps it in.
    const fn geo_property_members(&self) -> [(&'static str, &Option<M>); 3] {
        [
            (compact_properties::LOCATION, &self.location),
            (compact_properties::OBSERVATION_SPACE, &self.observation_space),
            (compact_properties::OPERATION_SPACE, &self.operation_space),
        ]
    }
}

impl Entity {
    /// Remove attribute instances whose `expiresAt` has passed (clause 4.22).
    ///
    /// Filters out expired instances from both regular attributes and the
    /// special geo-properties (location, observationSpace, operationSpace).
    /// If a multi-instance attribute has all instances expired, the attribute
    /// key is removed entirely.
    pub fn remove_expired_attributes(&mut self, now: DateTime<Utc>) {
        fn is_expired(attr: &Attribute, now: DateTime<Utc>) -> bool {
            attr.expires_at().is_some_and(|exp| exp <= now)
        }

        fn filter_value(value: &mut Option<AttributeValue>, now: DateTime<Utc>) {
            let should_remove = match value {
                Some(AttributeValue::Single(attr)) => is_expired(attr, now),
                Some(AttributeValue::Multiple(instances)) => {
                    instances.retain(|attr| !is_expired(attr, now));
                    instances.is_empty()
                }
                Some(AttributeValue::NgsiLdNull) | None => false,
            };
            if should_remove {
                *value = None;
            }
        }

        filter_value(&mut self.location, now);
        filter_value(&mut self.observation_space, now);
        filter_value(&mut self.operation_space, now);

        self.attrs.retain(|_, value| match value {
            AttributeValue::Single(attr) => !is_expired(attr, now),
            AttributeValue::Multiple(instances) => {
                instances.retain(|attr| !is_expired(attr, now));
                !instances.is_empty()
            }
            AttributeValue::NgsiLdNull => true,
        });
    }
}

/// Partial entity representation for merge/update operations.
///
/// According to NGSI-LD spec, merge and update operations accept a partial
/// entity representation where `id` and `type` are optional (since `id` is
/// in the URL path and `type` cannot be changed).
///
/// System-managed timestamps (`createdAt`, `modifiedAt`, `deletedAt`) are
/// excluded as they are not patchable. Decoding is
/// `crate::entity_decoding`'s, as the [`Entity`]'s is.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityPatch<M = AttributeValue> {
    /// Entity identifier, when the payload repeats it.
    pub id: Option<Identifier>,
    /// Entity types, when the payload repeats them.
    #[serde(rename = "type")]
    pub type_: Option<EntityTypeValue>,
    /// Scopes the entity belongs to (clause 4.18).
    pub scope: Option<ScopeMember>,

    /// Entity expiration timestamp (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,

    /// Default geospatial property (clause 4.7).
    pub location: Option<M>,

    /// Observation space geometry (clause 5.2.4).
    pub observation_space: Option<M>,

    /// Operation space geometry (clause 5.2.4).
    pub operation_space: Option<M>,

    /// Every other attribute the patch carries, keyed by attribute name.
    #[serde(flatten)]
    pub attrs: IndexMap<AttributeName, M>,
}

impl<M> EntityPatch<M> {
    /// Every Attribute member the patch carries with its name, in the order
    /// of [`EntityPatch::attribute_members`], each open to rewriting.
    pub fn attribute_members_mut(&mut self) -> impl Iterator<Item = (&str, &mut M)> {
        self.attrs.iter_mut().map(|(name, value)| (name.as_str(), value)).chain(
            [
                (compact_properties::LOCATION, &mut self.location),
                (compact_properties::OBSERVATION_SPACE, &mut self.observation_space),
                (compact_properties::OPERATION_SPACE, &mut self.operation_space),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.as_mut().map(|value| (name, value))),
        )
    }

    /// Every Attribute member the patch carries with its name, as
    /// [`Entity::attribute_members`] yields an entity's: [`EntityPatch::attrs`]
    /// in order, then `location`, `observationSpace` and `operationSpace` where
    /// present.
    pub fn attribute_members(&self) -> impl Iterator<Item = (&str, &M)> {
        self.attrs.iter().map(|(name, value)| (name.as_str(), value)).chain(
            [
                (compact_properties::LOCATION, &self.location),
                (compact_properties::OBSERVATION_SPACE, &self.observation_space),
                (compact_properties::OPERATION_SPACE, &self.operation_space),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.as_ref().map(|value| (name, value))),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::instance::AttributeValue,
        entity::{Entity, EntityPatch},
        notification::SysAttrsSnapshot,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier_length::IDENTIFIER_BYTE_LIMIT};
    use chrono::{Datelike, TimeZone, Utc};
    use serde_json::{Value, json};

    /// A value of exactly `length` octets that starts with `prefix`.
    fn of_length(prefix: &str, length: usize) -> String {
        format!("{prefix}{}", "x".repeat(length - prefix.len()))
    }

    /// The message a refused payload deserialises with.
    fn refusal(payload: Value) -> String {
        serde_json::from_value::<Entity>(payload)
            .map(|entity| entity.id.to_string())
            .expect_err("the payload must be refused")
            .to_string()
    }

    /// A member is found by name wherever the type keeps it: a geo-property in
    /// its own field, every other attribute in the map.
    #[test]
    fn a_member_is_found_by_name_in_its_field_or_the_map() {
        let entity: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:A:1",
            "type": "A",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
            "speed": {"type": "Property", "value": 1}
        }))
        .expect("an entity");
        assert!(matches!(entity.attribute_member("location"), Some(AttributeValue::Single(_))));
        assert!(matches!(entity.attribute_member("speed"), Some(AttributeValue::Single(_))));
        assert!(entity.attribute_member("observationSpace").is_none());
        assert!(entity.attribute_member("missing").is_none());
    }

    /// Clause 4.6.2 sets no length on a URI, so a `ListRelationship` member far
    /// longer than any identifier is part of a valid Entity.
    #[test]
    fn an_entity_whose_list_relationship_holds_a_six_thousand_octet_member_deserializes() {
        let member = of_length("urn:ngsi-ld:GtfsStopTimeUpdate:", 6000);
        let payload = json!({
            "id": "urn:ngsi-ld:GtfsRtTripUpdate:1",
            "type": "GtfsRtTripUpdate",
            "stopTimeUpdates": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:GtfsStopTimeUpdate:1", member]}
        });
        let entity: Entity = serde_json::from_value(payload).expect("an entity with a long reference");
        assert!(entity.attrs.contains_key("stopTimeUpdates"));
    }

    /// An Entity id is a key in the `entities` index, so it is held to the
    /// identifier bound: 1024 octets are accepted and 1025 refused, naming the
    /// length.
    #[test]
    fn an_entity_id_one_octet_past_the_identifier_bound_is_refused() {
        let at_bound = of_length("urn:ngsi-ld:Building:", IDENTIFIER_BYTE_LIMIT);
        let entity: Entity = serde_json::from_value(json!({"id": at_bound, "type": "Building"})).expect("an id at the bound");
        pretty_assertions::assert_eq!(entity.id.as_str().len(), IDENTIFIER_BYTE_LIMIT);

        let message = refusal(json!({"id": of_length("urn:ngsi-ld:Building:", IDENTIFIER_BYTE_LIMIT + 1), "type": "Building"}));
        assert!(message.contains("1025 bytes"), "{message}");
    }

    #[test]
    fn entity_deserializes_with_scope_string() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "scope": "/Madrid/Gardens/ParqueNorte"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with scope");
        assert!(entity.scope.is_some());
    }

    #[test]
    fn entity_deserializes_multi_instance_attribute_array() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:002",
            "type": "Building",
            "name": [
                { "type": "Property", "value": "Eiffel Tower" },
                { "type": "Property", "value": "Tour Eiffel", "datasetId": "urn:ngsi-ld:Dataset:french-name" }
            ]
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with array attrs");

        assert!(matches!(entity.attrs.get("name"), Some(AttributeValue::Multiple(_))));
    }

    #[test]
    fn entity_deserializes_language_and_json_property() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:003",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": { "en": "Main Street", "de": "Hauptstrasse" }
            },
            "jsonProperty": {
                "type": "JsonProperty",
                "json": { "id": "some-id", "ok": true }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with new attrs");
        assert!(entity.attrs.contains_key("street"));
        assert!(entity.attrs.contains_key("jsonProperty"));
    }

    #[test]
    fn entity_deserializes_concise_attribute_values() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:004",
            "type": "Building",
            "A1": "value of A1",
            "A2": 42
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with concise attrs");
        assert!(matches!(entity.attrs.get("A1"), Some(AttributeValue::Single(_))));
        assert!(matches!(entity.attrs.get("A2"), Some(AttributeValue::Single(_))));
    }

    #[test]
    fn entity_deserializes_with_expires_at() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:005",
            "type": "Building",
            "expiresAt": "2030-12-31T23:59:59Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with expiresAt");
        assert!(entity.expires_at.is_some());
        pretty_assertions::assert_eq!(entity.expires_at.unwrap().year(), 2030);
        assert!(!entity.attrs.contains_key("expiresAt"));
    }

    #[test]
    fn entity_deserializes_with_location() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:006",
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [13.3986, 52.5547] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with location");
        assert!(entity.location.is_some());
        assert!(!entity.attrs.contains_key("location"));
    }

    #[test]
    fn entity_deserializes_with_observation_space() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:007",
            "type": "Building",
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with observationSpace");
        assert!(entity.observation_space.is_some());
        assert!(!entity.attrs.contains_key("observationSpace"));
    }

    #[test]
    fn entity_deserializes_with_operation_space() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:008",
            "type": "Building",
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [8.6845, 50.1109] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with operationSpace");
        assert!(entity.operation_space.is_some());
        assert!(!entity.attrs.contains_key("operationSpace"));
    }

    #[test]
    fn entity_deserializes_with_sys_attrs() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:009",
            "type": "Building",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-06-15T12:30:00Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with sys attrs");
        assert!(entity.created_at.is_some());
        assert!(entity.modified_at.is_some());
        assert!(!entity.attrs.contains_key("createdAt"));
        assert!(!entity.attrs.contains_key("modifiedAt"));
    }

    #[test]
    fn entity_serializes_omits_none_fields() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:010",
            "type": "Building",
            "name": { "type": "Property", "value": "Test" }
        });

        let entity: Entity = serde_json::from_value(payload).expect("basic entity");
        let serialized = serde_json::to_value(&entity).expect("serialize");
        let obj = serialized.as_object().expect("is object");

        assert!(!obj.contains_key("expiresAt"));
        assert!(!obj.contains_key("location"));
        assert!(!obj.contains_key("observationSpace"));
        assert!(!obj.contains_key("operationSpace"));
        assert!(!obj.contains_key("createdAt"));
        assert!(!obj.contains_key("modifiedAt"));
        assert!(!obj.contains_key("deletedAt"));
    }

    #[test]
    fn entity_special_fields_not_in_attrs() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:012",
            "type": "Building",
            "expiresAt": "2030-12-31T23:59:59Z",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [1.0, 2.0] }
            },
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [3.0, 4.0] }
            },
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [5.0, 6.0] }
            },
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-06-15T12:30:00Z",
            "deletedAt": "2024-12-31T23:59:59Z",
            "temperature": { "type": "Property", "value": 25 }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with all fields");

        // Named fields must NOT leak into the flatten map
        assert!(!entity.attrs.contains_key("expiresAt"));
        assert!(!entity.attrs.contains_key("location"));
        assert!(!entity.attrs.contains_key("observationSpace"));
        assert!(!entity.attrs.contains_key("operationSpace"));
        assert!(!entity.attrs.contains_key("createdAt"));
        assert!(!entity.attrs.contains_key("modifiedAt"));
        assert!(!entity.attrs.contains_key("deletedAt"));

        assert!(entity.attrs.contains_key("temperature"));

        assert!(entity.expires_at.is_some());
        assert!(entity.location.is_some());
        assert!(entity.observation_space.is_some());
        assert!(entity.operation_space.is_some());
        assert!(entity.created_at.is_some());
        assert!(entity.modified_at.is_some());
        assert!(entity.deleted_at.is_some());
    }

    #[test]
    fn entity_patch_deserializes_new_fields() {
        let payload = json!({
            "expiresAt": "2030-12-31T23:59:59Z",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [1.0, 2.0] }
            },
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [3.0, 4.0] }
            },
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [5.0, 6.0] }
            },
            "temperature": { "type": "Property", "value": 25 }
        });

        let patch: EntityPatch = serde_json::from_value(payload).expect("entity patch with new fields");

        assert!(patch.expires_at.is_some());
        assert!(patch.location.is_some());
        assert!(patch.observation_space.is_some());
        assert!(patch.operation_space.is_some());
        assert!(patch.attrs.contains_key("temperature"));
        assert!(!patch.attrs.contains_key("expiresAt"));
        assert!(!patch.attrs.contains_key("location"));
    }

    #[test]
    fn entity_with_sys_attrs_populates_timestamps() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:013",
            "type": "Building"
        });

        let entity: Entity = serde_json::from_value(payload).expect("basic entity");
        assert!(entity.created_at.is_none());
        assert!(entity.modified_at.is_none());

        let snapshot = SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            modified_at: Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap(),
            expires_at: Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()),
        };

        let entity = entity.with_sys_attrs(&snapshot);
        pretty_assertions::assert_eq!(entity.created_at, Some(snapshot.created_at));
        pretty_assertions::assert_eq!(entity.modified_at, Some(snapshot.modified_at));
        pretty_assertions::assert_eq!(entity.expires_at, Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()));
    }

    #[test]
    fn entity_with_sys_attrs_preserves_existing_expires_at() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:014",
            "type": "Building",
            "expiresAt": "2025-06-01T00:00:00Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with expiresAt");
        let original_expires = entity.expires_at;

        let snapshot = SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            modified_at: Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap(),
            expires_at: Some(Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()),
        };

        let entity = entity.with_sys_attrs(&snapshot);
        // User-provided expiresAt is preserved, not overwritten by snapshot
        pretty_assertions::assert_eq!(entity.expires_at, original_expires);
    }

    fn entity_with_location() -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Vehicle:retain",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:Parking:1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }))
        .expect("entity with a location")
    }

    #[test]
    fn retaining_attribute_members_offers_exactly_the_member_names() {
        let mut entity = entity_with_location();
        let expected: Vec<String> = entity.attribute_member_names().map(str::to_owned).collect();
        let mut seen = Vec::new();

        entity.retain_attribute_members(|name, _member| {
            seen.push(name.to_owned());
            true
        });

        pretty_assertions::assert_eq!(seen, expected);
        pretty_assertions::assert_eq!(entity.attribute_member_names().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn retaining_attribute_members_drops_rejected_attributes_and_keeps_the_order_of_the_rest() {
        let mut entity = entity_with_location();
        let expected: Vec<String> = entity
            .attrs
            .keys()
            .map(AttributeName::as_str)
            .filter(|name| *name != "brandName")
            .map(str::to_owned)
            .collect();

        entity.retain_attribute_members(|name, _member| name != "brandName");

        let names: Vec<String> = entity.attrs.keys().map(AttributeName::as_str).map(str::to_owned).collect();
        pretty_assertions::assert_eq!(names, expected);
        pretty_assertions::assert_eq!(names.len(), 2);
        assert!(entity.location.is_some());
    }

    /// Clause 5.2.4 keeps `location` in its own member, and a narrowing that
    /// rejects it must reach it there.
    #[test]
    fn retaining_attribute_members_drops_a_rejected_geo_property() {
        let mut entity = entity_with_location();

        entity.retain_attribute_members(|name, _member| name != "location");

        assert!(entity.location.is_none());
        pretty_assertions::assert_eq!(entity.attrs.len(), 3);
    }

    #[test]
    fn retaining_attribute_members_offers_each_geo_property_member_itself() {
        let mut entity = entity_with_location();
        let mut location_kinds = Vec::new();

        entity.retain_attribute_members(|name, member| {
            if name == "location" {
                location_kinds.push(matches!(member, AttributeValue::Single(_)));
            }
            true
        });

        pretty_assertions::assert_eq!(location_kinds, vec![true]);
    }
}
