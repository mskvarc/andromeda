//! The typed entity response over a corpus of entities: what it writes for
//! every representation, `@context` carrier, `lang` filter, structural rule
//! and requested version, and what it says about itself in its head.
//!
//! The expectations are stated outright rather than taken from a second
//! reading of the same entity: clause 6.3.6 for where the `@context` goes,
//! clause 4.3.6.8 for the outcome of a requested version, clause 4.6.2 for the
//! Relationship targets and clause 4.5.16 for the `GeoJSON` envelope. The
//! version fallback tables themselves are asserted in
//! [`crate::entity_response_fallback_tests`].

use crate::{
    context::ResponseContext,
    entity_response::{EntityBody, EntityPayload},
    geometry_selection::GeometrySelection,
    response::{ResponseHead, ResponseRenderer},
    structural_projection::StructuralProjection,
    version::adaptation::VersionAdaptation,
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use iri_rs::IriBuf;
use serde_json::{Value, json};

/// The entities every combination is written over: each attribute family in
/// every shape it takes, the members carried verbatim, percent-encoded
/// Relationship targets in every place the presentation-boundary decoding
/// reaches, client values shaped like Relationships, an entity typed like a
/// Relationship family and one already carrying a Linked Entity.
fn corpus() -> Vec<Value> {
    [typed_entities(), verbatim_and_change_entities(), edge_entities()]
        .into_iter()
        .flatten()
        .collect()
}

/// The corpus entities whose attributes are typed instances of every family and shape.
fn typed_entities() -> Vec<Value> {
    vec![
        json!({"id": "urn:ngsi-ld:Building:minimal", "type": "Building"}),
        json!({
            "id": "urn:ngsi-ld:Building:families",
            "type": ["Building", "Landmark"],
            "scope": ["/Madrid/Centro", "/Europe"],
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-02T10:11:12.345Z",
            "expiresAt": "2030-01-01T00:00:00.123456Z",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "speed": {"type": "Property", "value": 12.50, "unitCode": "KMH", "observedAt": "2025-01-01T00:00:01Z", "valueType": "Speed"},
            "structured": {"type": "Property", "value": {"a": [1, 2.0, {"b": null}], "c": true}},
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:x%3A1", "objectType": "Person"},
            "owners": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:x%3A1", "urn:ngsi-ld:Person:2"]},
            "category": {"type": "VocabProperty", "vocab": ["commercial", "schema:Offer"]},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt", "@none": "Market"}, "observedAt": "2025-01-01T00:00:00Z"},
            "config": {"type": "JsonProperty", "json": {"type": "Relationship", "object": "urn%3Ajson%3Akept", "@context": "data"}},
            "readings": {"type": "ListProperty", "valueList": [1, "two", {"three": 3}]},
            "stops": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Stop:x%3A1"}, {"object": "urn:ngsi-ld:Stop:2"}], "objectType": "Stop"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}}
        }),
        json!({
            "id": "urn:ngsi-ld:Building:datasets",
            "type": "Building",
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [10.0, 20.0, 5.5]}},
                {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ],
            "observationSpace": {"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[0.0, 0.0], [3.0, 4.0]]}, "datasetId": "urn:ngsi-ld:Dataset:a"},
            "temperature": [
                {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 21},
                {"type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:a"}
            ],
            "label": [
                {"type": "LanguageProperty", "languageMap": {"de": "Haus", "en": "House"}},
                {"type": "LanguageProperty", "languageMap": {"@none": "untagged"}, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ]
        }),
        json!({
            "id": "urn:ngsi-ld:Building:nested",
            "type": "Building",
            "speed": {
                "type": "Property",
                "value": 10,
                "accuracy": {"type": "Property", "value": 0.5, "unitCode": "KMH"},
                "measuredBy": {"type": "Relationship", "object": "urn:ngsi-ld:Device:x%3A9", "observedAt": "2025-01-01T00:00:00Z"},
                "caption": {"type": "LanguageProperty", "languageMap": {"en": "speed"}},
                "readings": [
                    {"type": "Property", "value": 1},
                    {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:x"}
                ],
                "conciseNote": {"value": "a concise sub-attribute"},
                "plain": 5
            },
            "area": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 0.0]]]}, "source": {"type": "Property", "value": "survey"}},
            "position": {"type": "Property", "value": {"type": "Point", "coordinates": [7.0, 8.0]}},
            "track": {"type": "ListProperty", "valueList": [{"type": "Point", "coordinates": [1.0, 1.0]}, {"type": "Point", "coordinates": [2.0, 2.0]}]},
            "decodedInValue": {"type": "Property", "value": {"link": {"type": "Relationship", "object": "urn%3Ain%3Avalue"}}},
            "listInValue": {"type": "Property", "value": {"type": "ListRelationship", "objectList": ["urn%3Aa", {"object": "urn%3Ab", "type": "Relationship"}]}}
        }),
    ]
}

/// The corpus entities carrying concise members, showChanges members and instance members.
fn verbatim_and_change_entities() -> Vec<Value> {
    vec![
        json!({
            "id": "urn:ngsi-ld:Building:verbatim",
            "type": "Building",
            "conciseSpeed": {"value": 5},
            "conciseRelationship": {"object": "urn:ngsi-ld:X:x%3A1"},
            "conciseGeometry": {"type": "Point", "coordinates": [3.0, 4.0]},
            "plainNumber": 42,
            "plainText": "text",
            "rawArray": [{"value": 1}, {"value": 2, "datasetId": "urn:ngsi-ld:Dataset:raw"}]
        }),
        json!({
            "id": "urn:ngsi-ld:Building:changes",
            "type": "Building",
            "speed": {"type": "Property", "value": 11, "previousValue": 10},
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:2", "previousObject": "urn:ngsi-ld:Person:x%3A1"},
            "stops": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Stop:2"], "previousObjectList": ["urn:ngsi-ld:Stop:x%3A1"]},
            "deleted": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]},
            "category": {"type": "VocabProperty", "vocab": "commercial", "previousVocab": "private"},
            "config": {"type": "JsonProperty", "json": [1, 2], "previousJson": {"a": 1}},
            "street": {"type": "LanguageProperty", "languageMap": {"en": "new"}, "previousLanguageMap": {"en": "old"}},
            "readings": {"type": "ListProperty", "valueList": [2], "previousValueList": [1]},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}, "previousValue": {"type": "Point", "coordinates": [0.0, 0.0]}}
        }),
        json!({
            "id": "urn:ngsi-ld:Building:instance-members",
            "type": "Building",
            "speed": {
                "type": "Property",
                "value": 3,
                "createdAt": "2025-01-01T00:00:00Z",
                "modifiedAt": "2025-01-01T00:00:00.5Z",
                "expiresAt": "2031-01-01T00:00:00Z",
                "instanceId": "urn:ngsi-ld:attribute:instance:1",
                "datasetId": "urn:ngsi-ld:Dataset:only"
            },
            "location": {"type": "GeoProperty", "value": {"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]}, "datasetId": "urn:ngsi-ld:Dataset:only"},
            "stamped": {"type": "GeoProperty", "value": {"type": "MultiPoint", "coordinates": [[1.0, 2.0]]}, "dataset": {"type": "Property", "value": "named like the map"}}
        }),
    ]
}

/// The corpus entities exercising default instances, a type spelled like an
/// attribute family and a Linked Entity carried in.
fn edge_entities() -> Vec<Value> {
    vec![
        json!({
            "id": "urn:ngsi-ld:Building:defaults",
            "type": "Building",
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}}
            ],
            "operationSpace": {"type": "GeoProperty", "value": {"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]]]}}
        }),
        json!({
            "id": "urn:ngsi-ld:Relationship:typed-like-a-family",
            "type": "Relationship",
            "object": {"type": "Property", "value": "urn%3Akept%3Aas%3Aa%3Avalue"}
        }),
        json!({
            "id": "urn:ngsi-ld:Building:joined",
            "type": "Building",
            "owner": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Person:x%3A1",
                "entity": {"id": "urn:ngsi-ld:Person:1", "type": "Person", "name": {"type": "Property", "value": "Ana"}}
            }
        }),
    ]
}

/// The corpus as typed entities.
fn entities() -> Vec<Entity> {
    corpus()
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("a corpus entity decodes"))
        .collect()
}

/// The corpus entity named `id`.
fn entity(id: &str) -> Entity {
    entities().into_iter().find(|entity| entity.id.as_str() == id).expect("a corpus entity")
}

/// A projection keeping the given structural members only.
fn pick(terms: Vec<ProjectionTerm>) -> Projection {
    Projection { pick: terms, omit: Vec::new() }
}

/// A projection dropping the given structural members.
fn omit(terms: Vec<ProjectionTerm>) -> Projection {
    Projection { pick: Vec::new(), omit: terms }
}

/// Every clause 4.21 rule exercised.
fn projections() -> Vec<Option<Projection>> {
    let speed = ProjectionTerm::Attribute {
        iri: IriBuf::new("https://example.org/speed".to_owned()).expect("an IRI"),
        linked: Vec::new(),
    };
    vec![
        None,
        Some(pick(vec![ProjectionTerm::Id])),
        Some(pick(vec![speed])),
        Some(omit(vec![ProjectionTerm::Type, ProjectionTerm::Scope])),
        Some(pick(vec![ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope])),
    ]
}

/// Every clause 4.15 filter exercised.
fn language_filters() -> Vec<Option<LanguageFilter>> {
    vec![
        None,
        Some(LanguageFilter::parse("fr").expect("a filter")),
        Some(LanguageFilter::parse("*").expect("a filter")),
        Some(LanguageFilter::parse("ja").expect("a filter")),
    ]
}

/// The context both `@context` carriers hold: an embedded body or a `Link`.
fn embedded() -> ResponseContext {
    ResponseContext::Body(
        JsonLiteral::admit(
            &json!([
                "https://example.org/ctx.jsonld",
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
            ])
            .to_string(),
        )
        .expect("an admissible literal"),
    )
}

/// The `@context` carriers of clause 6.3.6, with the media type that uses
/// each.
fn carriers() -> Vec<(ResponseMediaType, ResponseContext)> {
    let link = || ResponseContext::Link(IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("an IRI"));
    vec![
        (ResponseMediaType::Json, link()),
        (ResponseMediaType::JsonLd, embedded()),
        (ResponseMediaType::GeoJson, embedded()),
        (ResponseMediaType::GeoJson, link()),
    ]
}

/// Every representation a response is written in.
const FORMATS: [ResponseFormat; 4] = [
    ResponseFormat::Normalized,
    ResponseFormat::Concise,
    ResponseFormat::KeyValues,
    ResponseFormat::Simplified,
];

/// Whether one entity or a collection is answered.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// One entity.
    Single,
    /// A collection.
    Collection,
}

/// One response configuration.
struct Writing<'a> {
    /// The media type and context carrier.
    carrier: &'a (ResponseMediaType, ResponseContext),
    /// The representation.
    format: ResponseFormat,
    /// The `lang` filter.
    language_filter: Option<&'a LanguageFilter>,
    /// The `Prefer: ngsi-ld=` version.
    version: Option<SpecVersion>,
    /// The geometry selection.
    selection: GeometrySelection<'a>,
}

impl<'a> Writing<'a> {
    /// The plain configuration of `carrier` in `format`.
    fn of(carrier: &'a (ResponseMediaType, ResponseContext), format: ResponseFormat) -> Writing<'a> {
        Writing {
            carrier,
            format,
            language_filter: None,
            version: None,
            selection: GeometrySelection::default(),
        }
    }

    /// The body and head written for `bodies`.
    fn write<'b>(&'b self, bodies: &[(&'b Entity, Option<StructuralProjection<'b>>)], shape: Shape) -> (Value, ResponseHead) {
        let body = |&(entity, projection): &(&'b Entity, Option<StructuralProjection<'b>>)| EntityBody::new(entity, projection);
        let payload = match shape {
            Shape::Single => EntityPayload::Single(bodies.first().map(body).expect("one entity")),
            Shape::Collection => EntityPayload::Collection(bodies.iter().map(body).collect()),
        };
        let mut written = Vec::new();
        let head = ResponseRenderer::new(self.carrier.0, self.carrier.1.clone()) // clone: each writing owns its context
            .with_format(self.format)
            .with_requested_version(self.version)
            .with_geometry_selection(self.selection)
            .with_entities(payload, self.language_filter)
            .render_into(&mut written)
            .expect("the entities are written");
        (serde_json::from_slice(&written).expect("the body is JSON"), head)
    }

    /// The body written for one entity.
    fn single(&self, entity: &Entity) -> (Value, ResponseHead) {
        self.write(&[(entity, None)], Shape::Single)
    }
}

/// Every entity is written, alone and as one collection, in every
/// representation under every carrier, `lang` filter and structural rule, and
/// each body is the JSON the shape asks for.
#[test]
fn every_entity_is_written_in_every_representation_and_option() {
    let entities = entities();
    for carrier in &carriers() {
        for format in FORMATS {
            for filter in &language_filters() {
                for projection in &projections() {
                    let writing = Writing {
                        language_filter: filter.as_ref(),
                        ..Writing::of(carrier, format)
                    };
                    let structural = projection.as_ref().map(StructuralProjection::from);
                    for entity in &entities {
                        let (body, head) = writing.write(&[(entity, structural)], Shape::Single);
                        let label = format!("{} as {:?} {format:?} lang={filter:?} projection={projection:?}", entity.id, carrier.0);
                        match carrier.0 {
                            ResponseMediaType::GeoJson => pretty_assertions::assert_eq!(body["type"], "Feature", "{label}"),
                            ResponseMediaType::Json | ResponseMediaType::JsonLd => assert!(body.is_object(), "{label}: {body}"),
                        }
                        pretty_assertions::assert_eq!(head.media_type, carrier.0, "{label}");
                    }
                    let collection: Vec<_> = entities.iter().map(|entity| (entity, structural)).collect();
                    let (body, _) = writing.write(&collection, Shape::Collection);
                    let label = format!("collection as {:?} {format:?} lang={filter:?} projection={projection:?}", carrier.0);
                    match carrier.0 {
                        ResponseMediaType::GeoJson => pretty_assertions::assert_eq!(body["type"], "FeatureCollection", "{label}"),
                        ResponseMediaType::Json | ResponseMediaType::JsonLd => {
                            pretty_assertions::assert_eq!(body.as_array().map(Vec::len), Some(entities.len()), "{label}");
                        }
                    }
                }
            }
        }
    }
}

/// Clause 6.3.6: an `application/ld+json` body embeds its `@context`, first, on
/// the entity or on every entity of a collection; an `application/json` body
/// embeds none and advertises it in one `Link`; a `GeoJSON` body embeds it on
/// the Feature or the `FeatureCollection` (clause 4.5.16) unless the `Link`
/// carries it.
#[test]
fn the_context_is_embedded_or_advertised_by_its_carrier() {
    let entities = entities();
    let building = &entities[1];
    for carrier in &carriers() {
        let writing = Writing::of(carrier, ResponseFormat::Normalized);
        let (single, head) = writing.single(building);
        let (collection, _) = writing.write(&[(building, None), (building, None)], Shape::Collection);
        match &carrier.1 {
            ResponseContext::Body(_) => {
                assert!(single.get("@context").is_some(), "{:?}: {single}", carrier.0);
                pretty_assertions::assert_eq!(head.context_link, None);
                match carrier.0 {
                    ResponseMediaType::GeoJson => assert!(collection.get("@context").is_some(), "{collection}"),
                    ResponseMediaType::Json | ResponseMediaType::JsonLd => {
                        let items = collection.as_array().expect("an array");
                        assert!(items.iter().all(|item| item.get("@context").is_some()), "{collection}");
                    }
                }
            }
            ResponseContext::Link(target) => {
                assert!(single.get("@context").is_none(), "{single}");
                pretty_assertions::assert_eq!(head.context_link.as_ref(), Some(target));
            }
            ResponseContext::Absent => {
                assert!(single.get("@context").is_none(), "{single}");
                pretty_assertions::assert_eq!(head.context_link, None);
            }
        }
    }
}

/// Clause 6.3.6: the `@context` member of an `application/ld+json` entity is a
/// member of the entity object itself, next to its own members.
#[test]
fn an_embedded_context_is_a_member_of_the_entity_object() {
    let carrier = (ResponseMediaType::JsonLd, embedded());
    let entity = entity("urn:ngsi-ld:Building:minimal");
    let mut written = Vec::new();
    ResponseRenderer::new(carrier.0, carrier.1)
        .with_entities(EntityPayload::Single(EntityBody::new(&entity, None)), None)
        .render_into(&mut written)
        .expect("the entity is written");
    let body: Value = serde_json::from_slice(&written).expect("the body is JSON");
    pretty_assertions::assert_eq!(body["id"], "urn:ngsi-ld:Building:minimal");
    pretty_assertions::assert_eq!(body["@context"][0], "https://example.org/ctx.jsonld");
}

/// The raw text one payload is written as under the embedded carrier of
/// `media_type`.
fn embedded_text(media_type: ResponseMediaType, format: ResponseFormat, payload: EntityPayload<'_>) -> String {
    let mut written = Vec::new();
    ResponseRenderer::new(media_type, embedded())
        .with_format(format)
        .with_entities(payload, None)
        .render_into(&mut written)
        .expect("the entities are written");
    String::from_utf8(written).expect("a JSON body is UTF-8")
}

/// Clause 6.3.6: `@context` is the first member of an entity in every
/// representation, of a `GeoJSON` Feature and of a `FeatureCollection`, and
/// of every item of an entity array.
#[test]
fn a_response_context_member_comes_first_in_every_entity_body() {
    let entity = entity("urn:ngsi-ld:Building:minimal");
    for format in FORMATS {
        let single = embedded_text(ResponseMediaType::JsonLd, format, EntityPayload::Single(EntityBody::new(&entity, None)));
        assert!(single.starts_with(r#"{"@context":"#), "{format:?}: {single}");
        let collection = embedded_text(
            ResponseMediaType::JsonLd,
            format,
            EntityPayload::Collection(vec![EntityBody::new(&entity, None), EntityBody::new(&entity, None)]),
        );
        assert!(collection.starts_with(r#"[{"@context":"#), "{format:?}: {collection}");
        assert!(collection.contains(r#"},{"@context":"#), "{format:?}: {collection}");
        let feature = embedded_text(ResponseMediaType::GeoJson, format, EntityPayload::Single(EntityBody::new(&entity, None)));
        assert!(feature.starts_with(r#"{"@context":"#), "{format:?}: {feature}");
        let features = embedded_text(
            ResponseMediaType::GeoJson,
            format,
            EntityPayload::Collection(vec![EntityBody::new(&entity, None)]),
        );
        assert!(features.starts_with(r#"{"@context":"#), "{format:?}: {features}");
    }
}

/// Clause 4.3.6.8: a conformant or newer version leaves the body as it is
/// (200), an older one adapts it (203), at every version the fallback tables
/// name, in every representation.
#[test]
fn a_requested_version_decides_the_outcome_and_a_current_one_changes_nothing() {
    let entities = entities();
    let carriers = carriers();
    for carrier in &carriers {
        for version in SpecVersion::SUPPORTED {
            for format in FORMATS {
                let adapted = Writing {
                    version: Some(version),
                    ..Writing::of(carrier, format)
                };
                let plain = Writing::of(carrier, format);
                for entity in &entities {
                    let (body, head) = adapted.single(entity);
                    pretty_assertions::assert_eq!(
                        head.version_adaptation,
                        VersionAdaptation::requested(version),
                        "{} {version:?} {format:?}",
                        entity.id
                    );
                    if head.version_adaptation == VersionAdaptation::Unchanged {
                        pretty_assertions::assert_eq!(body, plain.single(entity).0, "{} {version:?} {format:?}", entity.id);
                    }
                }
            }
        }
    }
}

/// Clause 4.6.2: the percent-encoded Relationship targets are written
/// decoded in every representation, and a client's own value -- a Property's
/// value, a `JsonProperty`'s `json` -- is never decoded, whatever it holds.
#[test]
fn relationship_targets_are_decoded_in_every_representation_and_values_are_not() {
    let families = entity("urn:ngsi-ld:Building:families");
    let nested = entity("urn:ngsi-ld:Building:nested");
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    for format in FORMATS {
        let writing = Writing::of(&carrier, format);
        let (body, _) = writing.single(&families);
        let text = body.to_string();
        assert!(text.contains("urn:ngsi-ld:Person:x:1"), "{format:?}: {text}");
        assert!(!text.contains("Person:x%3A1"), "{format:?}: {text}");
        assert!(text.contains("urn%3Ajson%3Akept"), "{format:?} keeps the JsonProperty's own value: {text}");
        let (body, _) = writing.single(&nested);
        assert!(body.to_string().contains("urn%3Ain%3Avalue"), "{format:?} keeps a Property's own value: {body}");
    }
}

/// Clause 4.6.2: only the targets of a Relationship and a `ListRelationship`
/// are decoded, in both spellings of an `objectList` member.
#[test]
fn list_relationship_targets_are_decoded_in_both_object_list_spellings() {
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    let (normalized, _) = Writing::of(&carrier, ResponseFormat::Normalized).single(&entity("urn:ngsi-ld:Building:families"));
    pretty_assertions::assert_eq!(
        normalized["stops"]["objectList"],
        json!([{"object": "urn:ngsi-ld:Stop:x:1"}, {"object": "urn:ngsi-ld:Stop:2"}])
    );
    let (simplified, _) = Writing::of(&carrier, ResponseFormat::Simplified).single(&entity("urn:ngsi-ld:Building:families"));
    pretty_assertions::assert_eq!(simplified["stops"], json!(["urn:ngsi-ld:Stop:x:1", "urn:ngsi-ld:Stop:2"]));
}

/// An entity typed like a Relationship family is still an entity: its own
/// members are values, and are not decoded.
#[test]
fn an_entity_typed_like_a_relationship_family_keeps_its_values() {
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    let (body, _) = Writing::of(&carrier, ResponseFormat::Normalized).single(&entity("urn:ngsi-ld:Relationship:typed-like-a-family"));
    pretty_assertions::assert_eq!(body["object"]["value"], "urn%3Akept%3Aas%3Aa%3Avalue");
}

/// Clause 4.21: a structural rule narrows `id`, `type` and `scope` entity by
/// entity, so a collection mixing rules is written by each entity's own.
#[test]
fn a_collection_mixing_projections_writes_each_entity_by_its_own_rule() {
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    let families = entity("urn:ngsi-ld:Building:families");
    let omitted = omit(vec![ProjectionTerm::Type, ProjectionTerm::Scope]);
    let kept = pick(vec![ProjectionTerm::Id, ProjectionTerm::Type]);
    let bodies = [
        (&families, Some(StructuralProjection::from(&omitted))),
        (&families, Some(StructuralProjection::from(&kept))),
        (&families, None),
    ];
    let (body, _) = Writing::of(&carrier, ResponseFormat::Normalized).write(&bodies, Shape::Collection);
    let items = body.as_array().expect("an array");
    assert!(items[0].get("type").is_none() && items[0].get("scope").is_none(), "{}", items[0]);
    assert!(items[0].get("name").is_some(), "{}", items[0]);
    assert!(items[1].get("type").is_some() && items[1].get("scope").is_none(), "{}", items[1]);
    assert!(items[2].get("type").is_some() && items[2].get("scope").is_some(), "{}", items[2]);
}

/// Clause 4.15: a `lang` filter negotiates every `LanguageProperty`, and a
/// filter that matches no language leaves the `@none` entry.
#[test]
fn a_lang_filter_negotiates_language_properties() {
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    let filter = LanguageFilter::parse("fr").expect("a filter");
    let writing = Writing {
        language_filter: Some(&filter),
        ..Writing::of(&carrier, ResponseFormat::Normalized)
    };
    let (body, _) = writing.single(&entity("urn:ngsi-ld:Building:families"));
    let text = body["street"].to_string();
    assert!(text.contains("Grand Place") && !text.contains("Grote Markt"), "{text}");
}

/// Clause 4.5.16.1: a `GeoJSON` Feature takes the geometry of the property the
/// request named.
#[test]
fn a_geojson_feature_takes_the_geometry_of_the_named_property() {
    let carrier = (ResponseMediaType::GeoJson, embedded());
    let property = "observationSpace".parse().expect("an attribute name");
    let writing = Writing {
        selection: GeometrySelection::new(Some(&property), &[]),
        ..Writing::of(&carrier, ResponseFormat::Normalized)
    };
    let (body, _) = writing.single(&entity("urn:ngsi-ld:Building:datasets"));
    pretty_assertions::assert_eq!(body["type"], "Feature");
    pretty_assertions::assert_eq!(body["geometry"]["type"], "LineString");
}

/// Clause 4.5.2.2: the `entityIdSealed` and `entityTypeSealed` members of an
/// `ngsildproof` Property are written bare, beside its `value`, in the
/// normalized and the concise representations alike; the concise one does not
/// collapse an instance that carries them.
#[test]
fn the_seals_of_an_ngsildproof_property_are_written_bare_in_both_reified_representations() {
    let sealed: Entity = serde_json::from_value(json!({
        "id": "urn:ngsi-ld:Store:002",
        "type": "Store",
        "address": {
            "type": "Property",
            "value": "x",
            "ngsildproof": {
                "type": "Property",
                "value": {"type": "DataIntegrityProof"},
                "entityIdSealed": "urn:ngsi-ld:Store:002",
                "entityTypeSealed": "Store"
            }
        }
    }))
    .expect("a sealed entity decodes");
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    for format in [ResponseFormat::Normalized, ResponseFormat::Concise] {
        let (body, _) = Writing::of(&carrier, format).single(&sealed);
        let proof = &body["address"]["ngsildproof"];
        pretty_assertions::assert_eq!(proof["entityIdSealed"], "urn:ngsi-ld:Store:002", "{format:?}: {body}");
        pretty_assertions::assert_eq!(proof["entityTypeSealed"], "Store", "{format:?}: {body}");
        pretty_assertions::assert_eq!(proof["value"]["type"], "DataIntegrityProof", "{format:?}: {body}");
    }
}

/// Clauses 4.5.21.2 and 4.7.3: the `unitCode` of a `ListProperty` and of a
/// `GeoProperty` is written as the bare member of the instance, in the
/// normalized and the concise representations alike.
#[test]
fn the_unit_code_of_a_list_property_and_a_geo_property_is_written_as_their_own_member() {
    let measured: Entity = serde_json::from_value(json!({
        "id": "urn:ngsi-ld:Building:measured",
        "type": "Building",
        "depths": {"type": "ListProperty", "valueList": [300, 120], "unitCode": "MMT"},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": "MMT"}
    }))
    .expect("an entity with units decodes");
    let carrier = (ResponseMediaType::Json, ResponseContext::Absent);
    for format in [ResponseFormat::Normalized, ResponseFormat::Concise] {
        let (body, _) = Writing::of(&carrier, format).single(&measured);
        pretty_assertions::assert_eq!(body["depths"]["unitCode"], "MMT", "{format:?}: {body}");
        pretty_assertions::assert_eq!(body["location"]["unitCode"], "MMT", "{format:?}: {body}");
    }
}

/// The corpus reaches every reading it is meant to: typed instances where a
/// family is written in full, concise members read as the Property they
/// abbreviate, and a Linked Entity carried in.
#[test]
fn the_corpus_reaches_every_reading() {
    let entities = entities();
    let member = |id: &str, name: &str| {
        let entity = entities.iter().find(|entity| entity.id.as_str() == id).expect("a corpus entity");
        entity
            .attribute_members()
            .find(|(member, _)| *member == name)
            .map(|(_, value)| value)
            .expect("a corpus member")
    };
    assert!(matches!(member("urn:ngsi-ld:Building:families", "owner"), AttributeValue::Single(attribute) if matches!(**attribute, Attribute::Relationship(_))));
    assert!(
        matches!(member("urn:ngsi-ld:Building:families", "stops"), AttributeValue::Single(attribute) if matches!(**attribute, Attribute::ListRelationship(_)))
    );
    assert!(matches!(member("urn:ngsi-ld:Building:datasets", "location"), AttributeValue::Multiple(_)));
    assert!(matches!(member("urn:ngsi-ld:Building:nested", "position"), AttributeValue::Single(_)));
    assert!(
        matches!(member("urn:ngsi-ld:Building:verbatim", "conciseSpeed"), AttributeValue::Single(attribute) if matches!(**attribute, Attribute::Property(_)))
    );
    assert!(matches!(member("urn:ngsi-ld:Building:verbatim", "rawArray"), AttributeValue::Multiple(_)));
    assert!(
        matches!(member("urn:ngsi-ld:Building:joined", "owner"), AttributeValue::Single(attribute) if matches!(&**attribute, Attribute::Relationship(owner) if owner.entity.is_some()))
    );
}
