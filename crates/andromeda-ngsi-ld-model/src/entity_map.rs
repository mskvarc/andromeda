//! The EntityMap resource that pins a distributed query's result set (ETSI GS CIM 009 clauses 4.5.25 and 5.2.39).

use andromeda_ngsi_ld_lexicon::{constant_string, rfc3339, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;
use typed_builder::TypedBuilder;

constant_string!(EntityMapType, "EntityMap");

/// The reserved `entityMap` value naming this broker as the holder of an entity.
pub const LOCAL_SOURCE_TOKEN: &str = "@none";

/// Where one entity of a map is held: locally, or in a registered context source
/// (Table 5.2.39-2 `entityMap` values).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum EntityMapSource {
    /// The entity is held by this broker; the wire token is `@none`.
    Local,
    /// The entity is held by the context source of this registration.
    Registration(Uri),
}

/// Why a string is not an [`EntityMapSource`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("entity map source must be @none or a registration URI, got {0}")]
pub struct EntityMapSourceError(String);

impl TryFrom<String> for EntityMapSource {
    type Error = EntityMapSourceError;

    fn try_from(raw: String) -> Result<EntityMapSource, EntityMapSourceError> {
        if raw == LOCAL_SOURCE_TOKEN {
            return Ok(EntityMapSource::Local);
        }
        raw.parse::<Uri>().map(EntityMapSource::Registration).map_err(|_| EntityMapSourceError(raw))
    }
}

// `@none` is a keyword, not a URI, so `derive_more::Display` would need a
// per-variant format and a constant it cannot name.
impl fmt::Display for EntityMapSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntityMapSource::Local => formatter.write_str(LOCAL_SOURCE_TOKEN),
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

/// NGSI-LD EntityMap representation (ETSI GS CIM 009 clause 5.2.39).
///
/// `entityMap` and `linkedMaps` are output-only: a client that supplies them
/// is ignored (Table 5.2.39-2), so deserialisation tolerates their absence and
/// the update operation never reads them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct EntityMap {
    /// Generated identifier of this map.
    pub id: Uri,
    /// Always `EntityMap`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: EntityMapType,
    /// When the map stops being usable for follow-up pages.
    #[serde(with = "rfc3339::datetime")]
    pub expires_at: DateTime<Utc>,
    /// Every entity the originating query matched, in order, each with the
    /// sources that hold it: `@none` for this broker, otherwise the registration
    /// whose context source returned it.
    #[serde(default)]
    #[builder(default = Default::default())]
    pub entity_map: IndexMap<Uri, Vec<EntityMapSource>>,
    /// The EntityMap each contributing context source created for the same
    /// query, keyed by the registration that named the source.
    #[serde(default)]
    #[builder(default = Default::default())]
    pub linked_maps: IndexMap<Uri, Uri>,
}

impl EntityMap {
    /// A map over entities this broker holds itself.
    pub fn local(id: Uri, expires_at: DateTime<Utc>, entity_ids: impl IntoIterator<Item = Uri>) -> EntityMap {
        let entity_map = entity_ids.into_iter().map(|entity_id| (entity_id, vec![EntityMapSource::Local])).collect();
        EntityMap::builder().id(id).expires_at(expires_at).entity_map(entity_map).build()
    }

    /// The identifiers of every entity in the map, in map order.
    pub fn entity_ids(&self) -> impl Iterator<Item = &Uri> {
        self.entity_map.keys()
    }

    /// Whether `entity_id` is one of the map's entities.
    pub fn contains(&self, entity_id: &Uri) -> bool {
        self.entity_map.contains_key(entity_id)
    }

    /// Every registration named as a source, without repetition, in map order.
    pub fn registrations(&self) -> impl Iterator<Item = &Uri> {
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

    /// The EntityMap a registration's context source created for this query.
    pub fn linked_map_for(&self, registration: &Uri) -> Option<&Uri> {
        self.linked_maps.get(registration)
    }

    /// Record that `registration` holds `entity_id`.
    fn add_source(&mut self, entity_id: Uri, source: EntityMapSource) {
        let sources = self.entity_map.entry(entity_id).or_default();
        if !sources.contains(&source) {
            sources.push(source);
        }
    }

    /// Fold in the entities a registration's context source returned for the
    /// same query, without a map of its own (clause 5.7.2.4 `entityMap=true`).
    pub fn add_remote_entities(&mut self, registration: &Uri, entity_ids: impl IntoIterator<Item = Uri>) {
        for entity_id in entity_ids {
            self.add_source(entity_id, EntityMapSource::Registration(registration.clone())); // clone: every entry owns its source
        }
    }

    /// Fold in the EntityMap a registration's context source created for the
    /// same query (clause 5.14.4.4): its identifier joins `linkedMaps`, and every
    /// entity it lists is attributed to the registration.
    pub fn add_remote(&mut self, registration: &Uri, remote: &EntityMap) {
        self.linked_maps.insert(registration.clone(), remote.id.clone()); // clone: the link owns both identifiers
        self.add_remote_entities(registration, remote.entity_ids().cloned()); // clone: the entries own their identifiers
    }

    /// Returns whether this map has already expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map::{EntityMap, EntityMapSource};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
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
            map.entity_map.get(&uri("urn:ngsi-ld:Vehicle:2")),
            Some(&vec![EntityMapSource::Local, EntityMapSource::Registration(registration.clone())]) // clone: compared below
        );
        pretty_assertions::assert_eq!(
            map.entity_map.get(&uri("urn:ngsi-ld:Vehicle:3")),
            Some(&vec![EntityMapSource::Registration(registration.clone())])
        ); // clone: compared below
        pretty_assertions::assert_eq!(map.registrations().collect::<Vec<_>>(), vec![&registration]);
        pretty_assertions::assert_eq!(map.entity_ids().count(), 3);
    }

    #[test]
    fn a_source_is_recorded_once_per_entity() {
        let mut map = local_map();
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:b");
        map.add_remote_entities(&registration, [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:1")]);
        pretty_assertions::assert_eq!(map.entity_map.get(&uri("urn:ngsi-ld:Vehicle:1")).map(Vec::len), Some(2));
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
            map.entity_map.get(&uri("urn:ngsi-ld:Vehicle:9")),
            Some(&vec![
                EntityMapSource::Local,
                EntityMapSource::Registration(uri("urn:ngsi-ld:ContextSourceRegistration:c"))
            ])
        );
        assert!(serde_json::from_value::<EntityMapSource>(json!("not a uri")).is_err());
    }

    #[test]
    fn a_map_without_output_members_still_parses() {
        let map: EntityMap =
            serde_json::from_value(json!({"id": "urn:ngsi-ld:EntityMap:bare", "type": "EntityMap", "expiresAt": "2030-01-01T00:00:00Z"})).expect("parses");
        assert!(map.entity_map.is_empty());
        assert!(map.linked_maps.is_empty());
    }
}
