//! The query languages read over a decoded state (clauses 4.9, 4.10, 4.19).

use crate::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry::Geometry, position::Position};
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, language_matching::LanguageMatching, parser::parse},
    geo::query::GeoQuery,
    queried_entity::linked_entities::NoLinkedEntities,
    scope::query::ScopeQuery,
};
use serde_json::{Value, json};
use std::sync::Arc;

const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
const RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/Relationship";
const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";
const DATE_TIME: &str = "https://uri.etsi.org/ngsi-ld/DateTime";

/// Decode a stored document into a state.
fn state(document: &Value) -> EntityState {
    EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes")
}

/// A vehicle with a value of every JSON scalar type, a Relationship and a
/// Property holding raw JSON.
fn vehicle() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Vehicle:A1",
        "@type": ["https://example.com/Vehicle"],
        "https://example.com/speed": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]}],
        "https://example.com/temperature": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 23.5}]}],
        "https://example.com/brandName": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Mercedes"}]}],
        "https://example.com/isParked": [{
            "@type": [RELATIONSHIP],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:OffStreetParking:Downtown1"}]
        }],
        "https://example.com/active": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": true}]}],
        "https://example.com/address": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@type": "@json", "@value": {"city": "Berlin", "country": "DE"}}]
        }],
        "https://example.com/reading": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "0.100000000000000005", "@type": "http://www.w3.org/2001/XMLSchema#decimal"}]
        }]
    }])
}

/// An entity of every attribute family, with a sub-attribute.
fn rich() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Device:1",
        "@type": ["https://example.com/Device"],
        "https://example.com/temperature": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}],
            "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": DATE_TIME, "@value": "2024-06-01T00:00:00Z"}]
        }],
        "https://example.com/description": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@language": "en", "@value": "Door"}, {"@language": "de", "@value": "Tuer"}]
        }],
        "https://example.com/tags": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}, {"@value": 2}, {"@value": 3}]
        }],
        "https://example.com/colors": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}, {"@value": "blue"}]}]
        }],
        "https://example.com/parts": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
            "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [{"@id": "urn:ngsi-ld:Part:1"}, {"@id": "urn:ngsi-ld:Part:2"}]}]
        }],
        "https://example.com/gender": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/VocabProperty"],
            "https://uri.etsi.org/ngsi-ld/hasVocab": [{"@id": "https://example.com/Male"}, {"@id": "https://example.com/Other"}]
        }],
        "https://example.com/owner": [{
            "@type": [RELATIONSHIP],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:42"}],
            "https://example.com/since": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2020}]}]
        }],
        "https://example.com/myJson": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/JsonProperty"],
            "https://uri.etsi.org/ngsi-ld/hasJSON": [{"@type": "@json", "@value": {"city": "Berlin"}}]
        }]
    }])
}

/// Two `datasetId` instances carrying the members a `.` segment can name, and
/// a structured value.
fn instance_members() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Sensor:1",
        "@type": ["https://example.com/Sensor"],
        "https://example.com/temperature": [
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}],
                "https://uri.etsi.org/ngsi-ld/createdAt": [{"@type": DATE_TIME, "@value": "2024-06-01T10:00:00.000000Z"}]
            },
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 35}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:b"}]
            }
        ],
        "https://example.com/address": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "https://example.com/city": [{"@value": "Berlin"}],
                "https://example.com/extra": [{"@type": "@json", "@value": {"floor": 3}}]
            }]
        }],
        "https://uri.etsi.org/ngsi-ld/location": [{
            "@type": [GEO_PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"}]
        }]
    }])
}

/// A stored document fixture.
type Fixture = fn() -> Value;

/// The Query Terms over [`vehicle`], with the answer clause 4.9 gives.
fn vehicle_cases() -> Vec<(&'static str, bool)> {
    vec![
        ("\"https://example.com/speed\"", true),
        ("\"https://example.com/nonexistent\"", false),
        ("\"https://example.com/speed\">50", true),
        ("\"https://example.com/speed\">100", false),
        ("\"https://example.com/speed\"==80", true),
        ("\"https://example.com/speed\"!=50", true),
        ("\"https://example.com/speed\"==\"80\"", true),
        ("\"https://example.com/speed\"~=\"8.*\"", false),
        ("\"https://example.com/temperature\"==23.5", true),
        ("\"https://example.com/reading\"==0.100000000000000005", true),
        ("\"https://example.com/reading\"==0.100000000000000004", false),
        ("\"https://example.com/brandName\"==\"Mercedes\"", true),
        ("\"https://example.com/brandName\"!=70..90", true),
        ("\"https://example.com/brandName\"==\"BMW\",\"Mercedes\"", true),
        ("\"https://example.com/brandName\"~=\"Merc.*\"", true),
        ("\"https://example.com/brandName\"!~=\"BMW.*\"", true),
        ("\"https://example.com/isParked\"==\"urn:ngsi-ld:OffStreetParking:Downtown1\"", true),
        ("\"https://example.com/isParked\">\"a\"", false),
        ("\"https://example.com/active\"==true", true),
        ("\"https://example.com/active\"!=5", true),
        ("\"https://example.com/speed\"==70..90", true),
        ("\"https://example.com/speed\">100|\"https://example.com/brandName\"==\"Mercedes\"", true),
        ("\"https://example.com/speed\">100;\"https://example.com/brandName\"==\"Mercedes\"", false),
        ("\"https://example.com/address\"[city]==\"Berlin\"", true),
        ("\"https://example.com/address\"[city]==\"Paris\"", false),
    ]
}

/// The Query Terms over [`rich`], with the answer clause 4.9 gives.
fn rich_cases() -> Vec<(&'static str, bool)> {
    vec![
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">2024-01-01T00:00:00Z",
            true,
        ),
        ("\"https://example.com/description\"", true),
        ("\"https://example.com/description\"==\"Tuer\"", true),
        ("\"https://example.com/description\"!=\"Tuer\"", false),
        ("\"https://example.com/description\"~=\"Do.*\"", true),
        ("\"https://example.com/description\"!~=\"rot.*\"", true),
        ("\"https://example.com/description\"[en]==\"Door\"", true),
        ("\"https://example.com/description\"[en]==\"Tuer\"", false),
        ("\"https://example.com/description\"[de]==\"Tuer\"", true),
        ("\"https://example.com/description\"[en]!=\"Tuer\"", true),
        ("\"https://example.com/description\"[fr]!=\"Door\"", true),
        ("\"https://example.com/description\"[de]~=\"Tu.*\"", true),
        ("\"https://example.com/description\"[en]", true),
        ("\"https://example.com/description\"[fr]", false),
        ("\"https://example.com/tags\"==3", true),
        ("\"https://example.com/tags\"!=3", false),
        ("\"https://example.com/colors\"==\"blue\"", true),
        ("\"https://example.com/colors\"~=\"re.*\"", false),
        ("\"https://example.com/parts\"==\"urn:ngsi-ld:Part:2\"", true),
        ("\"https://example.com/parts\"!=\"urn:ngsi-ld:Part:1\"", false),
        ("\"https://example.com/parts\">\"urn:ngsi-ld:Part:0\"", false),
        ("\"https://example.com/gender\"==\"https://example.com/Other\"", true),
        ("\"https://example.com/gender\"!=\"https://example.com/Male\"", false),
        ("\"https://example.com/owner\"~=\"urn:.*\"", false),
        ("\"https://example.com/owner\".\"https://example.com/since\"==2020", true),
        ("\"https://example.com/myJson\"[city]==\"Berlin\"", true),
    ]
}

/// The Query Terms over [`instance_members`], with the answer clause 4.9 gives.
fn instance_members_cases() -> Vec<(&'static str, bool)> {
    vec![
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/datasetId\"==\"urn:ngsi-ld:Dataset:b\"",
            true,
        ),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/datasetId\"==\"urn:ngsi-ld:Dataset:c\"",
            false,
        ),
        ("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/unitCode\"==\"CEL\"", true),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/createdAt\">2024-06-01T09:00:00Z",
            true,
        ),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/createdAt\">2024-06-01T11:00:00Z",
            false,
        ),
        ("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/modifiedAt\"", false),
        ("\"https://example.com/temperature\">30", true),
        ("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/hasValue\">30", true),
        ("\"https://example.com/address\"[\"https://example.com/city\"]==\"Berlin\"", true),
        (
            "\"https://example.com/address\".\"https://uri.etsi.org/ngsi-ld/hasValue\".\"https://example.com/city\"==\"Berlin\"",
            true,
        ),
        ("\"https://example.com/address\"[\"https://example.com/postcode\"]", false),
        (
            "\"https://uri.etsi.org/ngsi-ld/location\"==\"{\\\"type\\\":\\\"Point\\\",\\\"coordinates\\\":[13.4,52.5]}\"",
            true,
        ),
    ]
}

/// Every Query Term with the fixture it is evaluated over and the answer
/// clause 4.9 gives.
fn cases() -> Vec<(Fixture, Vec<(&'static str, bool)>)> {
    vec![(vehicle, vehicle_cases()), (rich, rich_cases()), (instance_members, instance_members_cases())]
}

/// Clause 4.9: every Query Term reads the decoded state as the clause answers
/// it.
#[test]
fn the_state_reading_answers_every_query_term_as_clause_4_9_does() {
    for (fixture, terms) in cases() {
        let entity = state(&fixture());
        for (q, expected) in terms {
            let expr = parse(q).expect("the Query Term parses");
            pretty_assertions::assert_eq!(evaluate(&expr, &entity, NoLinkedEntities, LanguageMatching::TagEquality), expected, "{q}");
        }
    }
}

/// Clause 4.9: a bare attribute path matches an entity that "define[s] the
/// target element ... regardless of any target value", so a structured
/// Property value and a geometry satisfy it.
#[test]
fn a_bare_path_holds_for_a_structured_value_and_a_geometry() {
    let mut document = instance_members();
    document[0]["https://example.com/site"] = json!([{
        "@type": [GEO_PROPERTY],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.4}, {"@value": 52.5}]}]
        }]
    }]);
    let entity = state(&document);
    for q in ["\"https://example.com/address\"", "\"https://example.com/site\""] {
        let expr = parse(q).expect("the Query Term parses");
        assert!(evaluate(&expr, &entity, NoLinkedEntities, LanguageMatching::TagEquality), "{q}");
    }
}

/// A point at the given longitude and latitude.
fn point(longitude: f64, latitude: f64) -> Geometry {
    Geometry::new(GeometryForm::Point(Position::try_new(longitude, latitude, None).expect("a CRS84 position")))
}

/// Clause 4.10: the geometry of the clause 4.7.2 string form is matched like
/// any other geometry.
#[test]
fn a_geo_query_reads_every_geometry_the_state_holds() {
    let entity = state(&instance_members());
    let near = |longitude, latitude| GeoQuery {
        georel: "near;maxDistance==1000".parse().expect("a georel"),
        geometry: point(longitude, latitude),
        geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name"),
    };
    assert!(near(13.4, 52.5).matches_entity(&entity));
    assert!(!near(2.35, 48.85).matches_entity(&entity));
    let elsewhere = GeoQuery {
        geoproperty: "https://example.com/address".parse().expect("an attribute name"),
        ..near(13.4, 52.5)
    };
    assert!(!elsewhere.matches_entity(&entity), "a Property holds no geometry");
}

/// Clause 4.19: a scope query reads the scopes the state belongs to.
#[test]
fn a_scope_query_reads_the_scopes_the_state_holds() {
    let mut document = vehicle();
    document[0]["https://uri.etsi.org/ngsi-ld/scope"] = json!([{"@value": "/Madrid/Gardens"}, {"@value": "/Berlin"}]);
    let scoped = state(&document);
    assert!(ScopeQuery::parse("/Madrid/#").expect("a scope query").matches_entity(&scoped));
    assert!(ScopeQuery::parse("/Berlin").expect("a scope query").matches_entity(&scoped));
    assert!(!ScopeQuery::parse("/Paris").expect("a scope query").matches_entity(&scoped));
    assert!(!ScopeQuery::parse("/Madrid/#").expect("a scope query").matches_entity(&state(&vehicle())));
}

/// Clause 4.9 EXAMPLE 13: a hop reads the stored state of the entity the
/// Relationship's object names.
#[test]
fn a_hop_reads_the_linked_state() {
    let vehicle = state(&vehicle());
    let parking = |category: &str| {
        state(&json!([{
            "@id": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "@type": ["https://example.com/OffStreetParking"],
            "https://example.com/category": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]}]
        }]))
    };
    let hop = parse("\"https://example.com/isParked\"{\"https://example.com/category\"}==\"commercial\"").expect("the Query Term parses");
    let mut commercial = LinkedStates::new();
    commercial.insert(Arc::new(parking("commercial")));
    let mut residential = LinkedStates::new();
    residential.insert(Arc::new(parking("residential")));
    assert!(evaluate(&hop, &vehicle, &commercial, LanguageMatching::TagEquality));
    assert!(!evaluate(&hop, &vehicle, &residential, LanguageMatching::TagEquality));
    assert!(!evaluate(&hop, &vehicle, NoLinkedEntities, LanguageMatching::TagEquality));
}
