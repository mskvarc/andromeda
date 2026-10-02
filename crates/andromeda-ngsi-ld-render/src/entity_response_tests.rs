//! The typed entity response writes, for every representation and option,
//! exactly the body the document pipeline renders from the same entity with
//! its Relationship targets decoded by the document rule of
//! [`crate::uri_decoding`]: the same JSON once parsed, member order aside (IETF
//! RFC 8259 section 4 makes it insignificant). The walk decodes each target as
//! it writes it, so the two agree only if both decode exactly the same members,
//! in every representation -- the simplified one included, whose document keeps
//! nothing that would tell a target apart.

use crate::{
    context::ResponseContext,
    entity::EntityRenderer,
    entity_response::{EntityBody, EntityPayload},
    geometry_selection::GeometrySelection,
    response::{ReadyResponse, ResponseHead, ResponsePayload, ResponseRenderer, TargetDecoding},
    structural_projection::StructuralProjection,
    uri_decoding::decode_percent_encoded_uris,
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    projection::{compiled::Projection, term::ProjectionTerm},
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use iri_rs::IriBuf;
use serde_json::{Value, json};

/// The entities every combination is rendered over: each attribute family in
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

/// The corpus entities carrying verbatim members, showChanges members and instance members.
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
            "rawArray": [{"value": 1}, {"value": 2, "datasetId": "urn:ngsi-ld:Dataset:raw"}],
            "unknownFamily": {"type": "Propertyy", "value": 1}
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

/// The `@context` carriers of clause 6.3.6, with the media type that uses
/// each.
fn carriers() -> Vec<(ResponseMediaType, ResponseContext)> {
    let link = || ResponseContext::Link(IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("an IRI"));
    let body = || {
        ResponseContext::Body(json!([
            "https://example.org/ctx.jsonld",
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
        ]))
    };
    vec![
        (ResponseMediaType::Json, link()),
        (ResponseMediaType::JsonLd, body()),
        (ResponseMediaType::GeoJson, body()),
        (ResponseMediaType::GeoJson, link()),
    ]
}

/// The clause 4.5.16.1 selections exercised: property name and `datasetId`
/// selectors.
fn selections() -> Vec<(Option<AttributeName>, Vec<DatasetSelector>)> {
    let name = |raw: &str| Some(raw.parse::<AttributeName>().expect("an attribute name"));
    let selectors = |raw: &[&str]| {
        raw.iter()
            .map(|selector| selector.parse().expect("a selector"))
            .collect::<Vec<DatasetSelector>>()
    };
    vec![
        (None, Vec::new()),
        (None, selectors(&["urn:ngsi-ld:Dataset:a"])),
        (None, selectors(&["urn:ngsi-ld:Dataset:b", "@none"])),
        (None, selectors(&["urn:ngsi-ld:Dataset:only"])),
        (name("observationSpace"), Vec::new()),
        (name("observationSpace"), selectors(&["urn:ngsi-ld:Dataset:z"])),
        (name("position"), Vec::new()),
        (name("track"), Vec::new()),
        (name("conciseGeometry"), Vec::new()),
        (name("stamped"), Vec::new()),
        (name("area"), Vec::new()),
        (name("rawArray"), Vec::new()),
    ]
}

/// One rendering configuration.
struct Rendering<'a> {
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

impl Rendering<'_> {
    /// The renderer this configuration starts from.
    fn renderer(&self) -> ResponseRenderer<'_> {
        ResponseRenderer::new(self.carrier.0, self.carrier.1.clone()) // clone: each rendering owns its context
            .with_format(self.format)
            .with_requested_version(self.version)
            .with_geometry_selection(self.selection)
    }

    /// The body and head the document pipeline renders: each entity's
    /// normalized document has its targets decoded by the document rule, is
    /// read back, and is rendered in the representation asked for.
    fn by_document(&self, bodies: &[(&Entity, Option<StructuralProjection<'_>>)], shape: Shape) -> (Value, ResponseHead) {
        let render = |(entity, projection): &(&Entity, Option<StructuralProjection<'_>>)| {
            let mut normalized = EntityRenderer::new(entity).render().expect("the document reading renders");
            decode_percent_encoded_uris(&mut normalized);
            let decoded: Entity = serde_json::from_value(normalized).expect("the decoded document reads back");
            EntityRenderer::new(&decoded)
                .with_optional_language_filter(self.language_filter)
                .with_mode(self.format)
                .with_optional_structural_projection(*projection)
                .render()
                .expect("the document reading renders")
        };
        let payload = match shape {
            Shape::Single => ResponsePayload::Single(bodies.first().map(render).expect("one entity")),
            Shape::Collection => ResponsePayload::Collection(bodies.iter().map(render).collect()),
        };
        let rendered = ReadyResponse {
            renderer: self.renderer(),
            payload,
            targets: TargetDecoding::Applied,
        }
        .render()
        .expect("the document pipeline renders");
        (rendered.value, rendered.head)
    }

    /// The body and head the typed pipeline writes.
    fn typed<'b>(&'b self, bodies: &[(&'b Entity, Option<StructuralProjection<'b>>)], shape: Shape) -> (Value, ResponseHead) {
        let body = |&(entity, projection): &(&'b Entity, Option<StructuralProjection<'b>>)| EntityBody::new(entity, projection);
        let payload = match shape {
            Shape::Single => EntityPayload::Single(bodies.first().map(body).expect("one entity")),
            Shape::Collection => EntityPayload::Collection(bodies.iter().map(body).collect()),
        };
        let mut written = Vec::new();
        let head = self
            .renderer()
            .with_entities(payload, self.language_filter)
            .render_into(&mut written)
            .expect("the typed pipeline writes");
        (serde_json::from_slice(&written).expect("the body is JSON"), head)
    }
}

/// Whether one entity or a collection is answered.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// One entity.
    Single,
    /// A collection.
    Collection,
}

/// Assert both pipelines agree on one configuration.
fn assert_agree<'b>(rendering: &'b Rendering<'b>, bodies: &[(&'b Entity, Option<StructuralProjection<'b>>)], shape: Shape, label: &str) {
    let (expected, expected_head) = rendering.by_document(bodies, shape);
    let (written, head) = rendering.typed(bodies, shape);
    pretty_assertions::assert_eq!(written, expected, "{label}");
    pretty_assertions::assert_eq!(head, expected_head, "{label}");
}

#[test]
fn every_entity_renders_alike_in_every_representation_and_option() {
    let entities = entities();
    let selections = selections();
    for carrier in &carriers() {
        let selections: &[_] = if carrier.0 == ResponseMediaType::GeoJson {
            &selections
        } else {
            &selections[..1]
        };
        for (property, selectors) in selections {
            for format in [
                ResponseFormat::Normalized,
                ResponseFormat::Concise,
                ResponseFormat::KeyValues,
                ResponseFormat::Simplified,
            ] {
                assert_every_option_agrees(&entities, carrier, format, GeometrySelection::new(property.as_ref(), selectors));
            }
        }
    }
}

/// Assert both pipelines agree on every entity, alone and as one collection,
/// under every `lang` filter and projection, in one carrier, representation and
/// geometry selection.
fn assert_every_option_agrees(entities: &[Entity], carrier: &(ResponseMediaType, ResponseContext), format: ResponseFormat, selection: GeometrySelection<'_>) {
    for filter in &language_filters() {
        for projection in &projections() {
            let rendering = Rendering {
                carrier,
                format,
                language_filter: filter.as_ref(),
                version: None,
                selection,
            };
            let structural = projection.as_ref().map(StructuralProjection::from);
            for entity in entities {
                let label = format!(
                    "{} as {:?} {format:?} lang={filter:?} projection={projection:?} selection={selection:?}",
                    entity.id, carrier.0
                );
                assert_agree(&rendering, &[(entity, structural)], Shape::Single, &label);
            }
            let collection: Vec<_> = entities.iter().map(|entity| (entity, structural)).collect();
            let label = format!(
                "collection as {:?} {format:?} lang={filter:?} projection={projection:?} selection={selection:?}",
                carrier.0
            );
            assert_agree(&rendering, &collection, Shape::Collection, &label);
        }
    }
}

/// A clause 4.5.23 flat join: each entity of one collection narrowed by the
/// rule that governs it.
#[test]
fn a_collection_mixing_projections_renders_alike() {
    let entities = entities();
    let projections = projections();
    let carriers = carriers();
    for carrier in &carriers {
        for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
            let rendering = Rendering {
                carrier,
                format,
                language_filter: None,
                version: None,
                selection: GeometrySelection::default(),
            };
            let collection: Vec<_> = entities
                .iter()
                .zip(projections.iter().cycle())
                .map(|(entity, projection)| (entity, projection.as_ref().map(StructuralProjection::from)))
                .collect();
            assert_agree(&rendering, &collection, Shape::Collection, &format!("{:?} {format:?}", carrier.0));
        }
    }
}

/// Clause 4.3.6.8: a conformant or newer version leaves the body as it is
/// (200), an older one adapts it (203); both agree with the document pipeline.
#[test]
fn a_requested_version_renders_alike() {
    let entities = entities();
    let carriers = carriers();
    for carrier in &carriers {
        for version in [SpecVersion::CONFORMANT, SpecVersion::V1_4] {
            for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
                let rendering = Rendering {
                    carrier,
                    format,
                    language_filter: None,
                    version: Some(version),
                    selection: GeometrySelection::default(),
                };
                for entity in &entities {
                    assert_agree(&rendering, &[(entity, None)], Shape::Single, &format!("{} {version:?} {format:?}", entity.id));
                }
                let collection: Vec<_> = entities.iter().map(|entity| (entity, None)).collect();
                assert_agree(&rendering, &collection, Shape::Collection, &format!("collection {version:?} {format:?}"));
            }
        }
    }
}

/// The corpus reaches every reading it is meant to: typed instances where a
/// family is written in full, verbatim members where one is not, and a
/// Linked Entity carried in.
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
    assert!(matches!(member("urn:ngsi-ld:Building:verbatim", "conciseSpeed"), AttributeValue::Raw(_)));
    assert!(matches!(member("urn:ngsi-ld:Building:verbatim", "rawArray"), AttributeValue::Raw(_)));
    assert!(
        matches!(member("urn:ngsi-ld:Building:joined", "owner"), AttributeValue::Single(attribute) if matches!(&**attribute, Attribute::Relationship(owner) if owner.entity.is_some()))
    );
}
