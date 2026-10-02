//! The query languages read over a written Temporal Evolution document answer
//! as clauses 4.9, 4.10, 4.19 and 5.6.11 read it.

use crate::document::EntityDocument;
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry::Geometry, position::Position};
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, parser::parse},
    geo::query::GeoQuery,
    scope::query::ScopeQuery,
};
use serde_json::{Value, json};

const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";

/// A Temporal Evolution fragment: two instances of one `datasetId`, a
/// typeless instance, a deleted instance, a geometry and a scope.
fn evolution() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Sensor:1",
        "@type": ["https://example.com/Sensor"],
        "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid/Gardens"}],
        "https://example.com/temperature": [
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T00:00:00Z"}],
                "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": "urn:ngsi-ld:Instance:1"}]
            },
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 35}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-02T00:00:00Z"}],
                "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": "urn:ngsi-ld:Instance:2"}]
            }
        ],
        "https://example.com/owner": [{"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:42"}]}],
        "https://example.com/humidity": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        }],
        "https://uri.etsi.org/ngsi-ld/location": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.4}, {"@value": 52.5}]}]
            }]
        }]
    }])
}

/// Decode the fragment as the document it is.
fn document() -> EntityDocument {
    EntityDocument::from_slice(&serde_json::to_vec(&evolution()).expect("JSON")).expect("a written document decodes")
}

/// Clause 4.9 over every written instance, several of one `datasetId`
/// included; a typeless `object` is a Relationship (clause 4.5.3).
#[test]
fn the_document_reading_answers_every_written_instance() {
    let written = document();
    for (q, expected) in [
        ("\"https://example.com/temperature\"==35", true),
        ("\"https://example.com/temperature\"==21", true),
        ("\"https://example.com/temperature\">40", false),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-06-01T12:00:00Z\"",
            true,
        ),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/instanceId\"==\"urn:ngsi-ld:Instance:2\"",
            true,
        ),
        ("\"https://example.com/owner\"==\"urn:ngsi-ld:Person:42\"", true),
        ("\"https://example.com/owner\">\"a\"", false),
        ("\"https://example.com/missing\"", false),
    ] {
        let expr = parse(q).expect("the Query Term parses");
        pretty_assertions::assert_eq!(evaluate(&expr, &written), expected, "{q}");
    }
}

/// A deletion holds no value to compare, though the written element is there
/// (clause 4.9 existence).
#[test]
fn a_deleted_instance_holds_no_value() {
    let written = document();
    assert!(!evaluate(
        &parse("\"https://example.com/humidity\"==\"urn:ngsi-ld:null\"").expect("parses"),
        &written
    ));
    assert!(evaluate(&parse("\"https://example.com/humidity\"").expect("parses"), &written));
}

/// Clauses 4.10 and 4.19 over the geometry and the scope the document writes.
#[test]
fn a_geo_query_and_a_scope_query_read_what_the_document_writes() {
    let written = document();
    let near = GeoQuery {
        georel: "near;maxDistance==1000".parse().expect("a georel"),
        geometry: Geometry::new(GeometryForm::Point(Position::try_new(13.4, 52.5, None).expect("a CRS84 position"))),
        geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name"),
    };
    assert!(near.matches_entity(&written));
    assert!(ScopeQuery::parse("/Madrid/#").expect("a scope query").matches_entity(&written));
    assert!(!ScopeQuery::parse("/Berlin").expect("a scope query").matches_entity(&written));
}
