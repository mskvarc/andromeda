//! The `EntityMap` resource that pins a distributed query's result set (ETSI GS CIM 009 clauses 4.5.25 and 5.2.39).

use crate::entity_map_fingerprint::RequestFingerprint;
use andromeda_ngsi_ld_lexicon::{
    constant_string,
    identifier::{Identifier, IdentifierParseError},
    time::instant_serde,
};
use andromeda_ngsi_ld_vocabulary::keyword;
use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use std::fmt;
use typed_builder::TypedBuilder;

constant_string!(EntityMapType, "EntityMap");

/// Where one entity of a map is held: locally, or in a registered context source
/// (Table 5.2.39-2 `entityMap` values).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum EntityMapSource {
    /// The entity is held by this broker; the wire token is `@none`.
    Local,
    /// The entity is held by the context source of this registration.
    Registration(Identifier),
}

/// Why a string is not an [`EntityMapSource`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("entity map source must be @none or a registration URI: {0}")]
pub struct EntityMapSourceError(#[source] IdentifierParseError);

impl TryFrom<String> for EntityMapSource {
    type Error = EntityMapSourceError;

    fn try_from(raw: String) -> Result<EntityMapSource, EntityMapSourceError> {
        if raw == keyword::NONE {
            return Ok(EntityMapSource::Local);
        }
        raw.parse::<Identifier>().map(EntityMapSource::Registration).map_err(EntityMapSourceError)
    }
}

// `@none` is a keyword, not a URI, so `derive_more::Display` would need a
// per-variant format and a constant it cannot name.
impl fmt::Display for EntityMapSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntityMapSource::Local => formatter.write_str(keyword::NONE),
            EntityMapSource::Registration(uri) => write!(formatter, "{uri}"),
        }
    }
}

// The wire form is produced by `Display`; `#[serde(into = "String")]` needs the
// owned conversion and cannot use it directly.
impl From<EntityMapSource> for String {
    fn from(source: EntityMapSource) -> String {
        source.to_string()
    }
}

/// NGSI-LD `EntityMap` representation (ETSI GS CIM 009 clause 5.2.39).
///
/// `entityMap` and `linkedMaps` are output-only: a client that supplies them
/// is ignored (Table 5.2.39-2), so deserialisation tolerates their absence and
/// the update operation never reads them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct EntityMap {
    /// Generated identifier of this map.
    pub id: Identifier,
    /// Always `EntityMap`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: EntityMapType,
    /// When the map stops being usable for follow-up pages.
    #[serde(with = "instant_serde::required")]
    pub expires_at: DateTime<Utc>,
    /// Every entity the originating query matched, in order, each with the
    /// sources that hold it, in the order they were found: `@none` for this
    /// broker, otherwise the registration whose context source returned it.
    ///
    /// Table 5.2.39-2 lists each holder of an entity once, so a source named
    /// twice for one entity, as a peer's map may do, is kept once.
    #[serde(default)]
    #[builder(default = Default::default())]
    pub entity_map: IndexMap<Identifier, IndexSet<EntityMapSource>>,
    /// The `EntityMap` each contributing context source created for the same
    /// query, keyed by the registration that named the source.
    #[serde(default)]
    #[builder(default = Default::default())]
    pub linked_maps: IndexMap<Identifier, Identifier>,
    /// The request that created the map, kept so a follow-up under it can be
    /// held to the same parameters (clause 5.5.14). Broker-internal: never
    /// part of the clause 5.2.39 representation.
    #[serde(skip)]
    #[builder(default = Default::default())]
    pub request_fingerprint: Option<RequestFingerprint>,
}

impl EntityMap {
    /// A map over entities this broker holds itself.
    pub fn local(id: Identifier, expires_at: DateTime<Utc>, entity_ids: impl IntoIterator<Item = Identifier>) -> EntityMap {
        let entity_map = entity_ids
            .into_iter()
            .map(|entity_id| (entity_id, IndexSet::from([EntityMapSource::Local])))
            .collect();
        EntityMap::builder().id(id).expires_at(expires_at).entity_map(entity_map).build()
    }

    /// The identifiers of every entity in the map, in map order.
    pub fn entity_ids(&self) -> impl Iterator<Item = &Identifier> {
        self.entity_map.keys()
    }

    /// Whether `entity_id` is one of the map's entities.
    #[must_use]
    pub fn contains(&self, entity_id: &Identifier) -> bool {
        self.entity_map.contains_key(entity_id)
    }

    /// Every registration named as a source, without repetition, in map order.
    pub fn registrations(&self) -> impl Iterator<Item = &Identifier> {
        let mut seen = Vec::new();
        self.entity_map
            .values()
            .flatten()
            .filter_map(|source| match source {
                EntityMapSource::Registration(registration) => Some(registration),
                EntityMapSource::Local => None,
            })
            .filter(move |registration| {
                if seen.contains(registration) {
                    false
                } else {
                    seen.push(*registration);
                    true
                }
            })
    }

    /// The `EntityMap` a registration's context source created for this query.
    #[must_use]
    pub fn linked_map_for(&self, registration: &Identifier) -> Option<&Identifier> {
        self.linked_maps.get(registration)
    }

    /// Record that `source` holds `entity_id`; a holder already recorded for
    /// the entity keeps its place.
    fn add_source(&mut self, entity_id: Identifier, source: EntityMapSource) {
        self.entity_map.entry(entity_id).or_default().insert(source);
    }

    /// Record that this broker holds `entity_ids` (clause 5.2.39 `@none`).
    pub fn add_local_entities(&mut self, entity_ids: impl IntoIterator<Item = Identifier>) {
        for entity_id in entity_ids {
            self.add_source(entity_id, EntityMapSource::Local);
        }
    }

    /// Fold in the entities a registration's context source returned for the
    /// same query, without a map of its own (clause 5.7.2.4 `entityMap=true`).
    pub fn add_remote_entities(&mut self, registration: &Identifier, entity_ids: impl IntoIterator<Item = Identifier>) {
        for entity_id in entity_ids {
            self.add_source(entity_id, EntityMapSource::Registration(registration.clone())); // clone: every entry owns its source
        }
    }

    /// Record the `EntityMap` a registration's context source used for this
    /// query (Table 5.2.39-2 `linkedMaps`), replacing any link already recorded.
    pub fn link(&mut self, registration: Identifier, linked_map: Identifier) {
        self.linked_maps.insert(registration, linked_map);
    }

    /// Fold in what one registration's context source contributed to a read
    /// (clauses 5.7.1.4 to 5.7.4.4): the entities it returned are attributed
    /// to it, and the map it answered with, if any, joins `linkedMaps`.
    pub fn add_contribution(&mut self, registration: &Identifier, entity_ids: impl IntoIterator<Item = Identifier>, linked_map: Option<Identifier>) {
        self.add_remote_entities(registration, entity_ids);
        if let Some(linked_map) = linked_map {
            self.link(registration.clone(), linked_map); // clone: the link owns the registration identifier
        }
    }

    /// Fold in the `EntityMap` a registration's context source created for the
    /// same query (clause 5.14.4.4): its identifier joins `linkedMaps`, and every
    /// entity it lists is attributed to the registration.
    pub fn add_remote(&mut self, registration: &Identifier, remote: &EntityMap) {
        self.link(registration.clone(), remote.id.clone()); // clone: the link owns both identifiers
        self.add_remote_entities(registration, remote.entity_ids().cloned()); // clone: the entries own their identifiers
    }

    /// Returns whether this map has already expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity_map::{EntityMap, EntityMapSource},
        entity_map_fingerprint::{MapRequestKind, RequestFingerprint},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    /// The sources recorded for `entity_id`, in the order the map keeps them.
    fn sources_of<'m>(map: &'m EntityMap, entity_id: &str) -> Vec<&'m EntityMapSource> {
        map.entity_map.get(&uri(entity_id)).map(|sources| sources.iter().collect()).unwrap_or_default()
    }

    fn local_map() -> EntityMap {
        EntityMap::local(
            uri("urn:ngsi-ld:EntityMap:1"),
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("valid instant"),
            [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")],
        )
    }

    #[test]
    fn a_local_map_renders_the_spec_shape_with_no_ids_member() {
        let rendered = serde_json::to_value(local_map()).expect("serialises");
        pretty_assertions::assert_eq!(
            rendered,
            json!({
                "id": "urn:ngsi-ld:EntityMap:1",
                "type": "EntityMap",
                "expiresAt": "2030-01-01T00:00:00Z",
                "entityMap": {"urn:ngsi-ld:Vehicle:1": ["@none"], "urn:ngsi-ld:Vehicle:2": ["@none"]},
                "linkedMaps": {}
            })
        );
        assert!(rendered.get("ids").is_none());
    }

    #[test]
    fn a_remote_map_adds_a_link_and_attributes_its_entities_to_the_registration() {
        let mut map = local_map();
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:b");
        let remote = EntityMap::local(
            uri("urn:ngsi-ld:EntityMap:remote"),
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("valid instant"),
            [uri("urn:ngsi-ld:Vehicle:2"), uri("urn:ngsi-ld:Vehicle:3")],
        );
        map.add_remote(&registration, &remote);
        pretty_assertions::assert_eq!(map.linked_map_for(&registration), Some(&uri("urn:ngsi-ld:EntityMap:remote")));
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:2"),
            vec![&EntityMapSource::Local, &EntityMapSource::Registration(registration.clone())] // clone: compared below
        );
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:3"),
            vec![&EntityMapSource::Registration(registration.clone())] // clone: compared below
        );
        pretty_assertions::assert_eq!(map.registrations().collect::<Vec<_>>(), vec![&registration]);
        pretty_assertions::assert_eq!(map.entity_ids().count(), 3);
    }

    #[test]
    fn a_source_is_recorded_once_per_entity() {
        let mut map = local_map();
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:b");
        map.add_remote_entities(&registration, [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:1")]);
        map.add_local_entities([uri("urn:ngsi-ld:Vehicle:1")]);
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:1"),
            vec![&EntityMapSource::Local, &EntityMapSource::Registration(registration.clone())] // clone: compared below
        );
    }

    #[test]
    fn a_contribution_attributes_its_entities_and_links_the_map_it_answered_with() {
        let mut map = local_map();
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:b");
        map.add_contribution(&registration, [uri("urn:ngsi-ld:Vehicle:3")], Some(uri("urn:ngsi-ld:EntityMap:b")));
        pretty_assertions::assert_eq!(map.linked_map_for(&registration), Some(&uri("urn:ngsi-ld:EntityMap:b")));
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:3"),
            vec![&EntityMapSource::Registration(registration.clone())] // clone: compared below
        );
        let other = uri("urn:ngsi-ld:ContextSourceRegistration:c");
        map.add_contribution(&other, [uri("urn:ngsi-ld:Vehicle:1")], None);
        assert!(map.linked_map_for(&other).is_none());
        // Registrations are listed in map order: Vehicle:1 names `c` before Vehicle:3 names `b`.
        pretty_assertions::assert_eq!(map.registrations().collect::<Vec<_>>(), vec![&other, &registration]);
    }

    #[test]
    fn the_request_fingerprint_never_reaches_the_wire() {
        let mut map = local_map();
        map.request_fingerprint = Some(RequestFingerprint::new(MapRequestKind::QueryEntities, [7; 32]));
        let rendered = serde_json::to_value(&map).expect("serialises");
        assert!(rendered.get("requestFingerprint").is_none());
        assert!(rendered.get("request_fingerprint").is_none());
    }

    #[test]
    fn the_wire_form_of_a_peer_map_parses_with_registration_sources() {
        let map: EntityMap = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:EntityMap:peer",
            "type": "EntityMap",
            "expiresAt": "2030-01-01T00:00:00Z",
            "entityMap": {"urn:ngsi-ld:Vehicle:9": ["@none", "urn:ngsi-ld:ContextSourceRegistration:c"]},
            "linkedMaps": {"urn:ngsi-ld:ContextSourceRegistration:c": "urn:ngsi-ld:EntityMap:c"}
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:9"),
            vec![
                &EntityMapSource::Local,
                &EntityMapSource::Registration(uri("urn:ngsi-ld:ContextSourceRegistration:c"))
            ]
        );
        assert!(serde_json::from_value::<EntityMapSource>(json!("not a uri")).is_err());
    }

    /// Table 5.2.39-2 lists each holder of an entity once: a peer's map naming
    /// one twice is read as naming it once, at its first place.
    #[test]
    fn a_peer_map_naming_a_holder_twice_for_one_entity_keeps_it_once_at_its_first_place() {
        let map: EntityMap = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:EntityMap:peer",
            "type": "EntityMap",
            "expiresAt": "2030-01-01T00:00:00Z",
            "entityMap": {"urn:ngsi-ld:Vehicle:9": ["urn:ngsi-ld:ContextSourceRegistration:c", "@none", "urn:ngsi-ld:ContextSourceRegistration:c"]}
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(
            sources_of(&map, "urn:ngsi-ld:Vehicle:9"),
            vec![
                &EntityMapSource::Registration(uri("urn:ngsi-ld:ContextSourceRegistration:c")),
                &EntityMapSource::Local
            ]
        );
        pretty_assertions::assert_eq!(
            serde_json::to_value(&map).expect("serialises")["entityMap"],
            json!({"urn:ngsi-ld:Vehicle:9": ["urn:ngsi-ld:ContextSourceRegistration:c", "@none"]})
        );
    }

    /// Table 5.2.39-2: a map with local and remote holders and a linked map
    /// decodes to the typed model and writes back the body it was parsed from.
    #[test]
    fn an_entity_map_round_trips_through_its_typed_model() {
        let body = json!({
            "id": "urn:ngsi-ld:EntityMap:round-trip",
            "type": "EntityMap",
            "expiresAt": "2030-01-01T00:00:00Z",
            "entityMap": {
                "urn:ngsi-ld:Vehicle:9": ["@none", "urn:ngsi-ld:ContextSourceRegistration:c"],
                "urn:ngsi-ld:Vehicle:10": ["urn:ngsi-ld:ContextSourceRegistration:c"]
            },
            "linkedMaps": {"urn:ngsi-ld:ContextSourceRegistration:c": "urn:ngsi-ld:EntityMap:c"}
        });
        let map: EntityMap = serde_json::from_value(body.clone()).expect("the body decodes to the typed EntityMap");
        pretty_assertions::assert_eq!(serde_json::to_value(&map).expect("the map serialises"), body);
    }

    #[test]
    fn a_map_without_output_members_still_parses() {
        let map: EntityMap =
            serde_json::from_value(json!({"id": "urn:ngsi-ld:EntityMap:bare", "type": "EntityMap", "expiresAt": "2030-01-01T00:00:00Z"})).expect("parses");
        assert!(map.entity_map.is_empty());
        assert!(map.linked_maps.is_empty());
    }
}
