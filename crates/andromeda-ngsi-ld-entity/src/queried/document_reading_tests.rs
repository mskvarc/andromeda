//! The query languages read over a written Temporal Evolution document answer
//! as clauses 4.9, 4.10, 4.19 and 5.6.11 read it.

use crate::document::EntityDocument;
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry::Geometry, position::Position};
use andromeda_ngsi_ld_query::{
    filter::{language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading},
    geo::query::GeoQuery,
    scope::query::ScopeQuery,
};
use andromeda_ngsi_ld_query_evaluation::{
    geo_query::geo_query_matches,
    q::filter_run::filter_holds,
    queried_entity::{linked_entities::NoLinkedEntities, scoped_entity::ScopedEntity},
    scope_query::scope_query_matches,
};
use serde_json::{Value, json};

const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";

/// The NGSI-LD Null of clause 4.5.0, the deletion encoding of every family.
const NULL: &str = "urn:ngsi-ld:null";

/// A Temporal Evolution fragment: two instances of one `datasetId`, a
/// typeless instance, deletions of a Property, a `ListProperty`, a
/// `ListRelationship` and a `LanguageProperty`, a structured value deleting
/// one member and holding the empty array in another, a geometry and a scope.
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
        "https://example.com/label": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@language": "en", "@value": "Gate"}, {"@language": "fr", "@value": "Portail"}]
        }],
        "https://example.com/humidity": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": NULL}]
        }],
        "https://example.com/levels": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": NULL}]}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]
        }],
        "https://example.com/route": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
            "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [{"@value": NULL}]}]
        }],
        "https://example.com/title": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@value": NULL}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]
        }],
        "https://example.com/address": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "https://example.com/street": [{"@value": NULL}],
                "https://example.com/tags": []
            }]
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
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">2024-06-01T12:00:00Z",
            true,
        ),
        (
            "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/instanceId\"==\"urn:ngsi-ld:Instance:2\"",
            true,
        ),
        ("\"https://example.com/owner\"==\"urn:ngsi-ld:Person:42\"", true),
        ("\"https://example.com/owner\">\"a\"", false),
        ("\"https://example.com/missing\"", false),
        ("\"https://example.com/label\"[fr]==\"Portail\"", true),
        ("\"https://example.com/label\"[en]==\"Portail\"", false),
        ("\"https://example.com/label\"[de]", false),
    ] {
        let expr = parse(q).expect("the Query Term parses");
        pretty_assertions::assert_eq!(
            filter_holds(&FilterReading::of(&expr, LanguageMatching::TagEquality), &written, NoLinkedEntities),
            expected,
            "{q}"
        );
    }
}

/// A deletion of any family holds no value to compare, so neither a
/// comparison nor its complement holds over it, with or without a target
/// language, though the written element is there (clause 4.9 existence).
#[test]
fn a_deleted_instance_of_any_family_holds_no_value() {
    let written = document();
    for (q, expected) in [
        ("\"https://example.com/humidity\"==\"urn:ngsi-ld:null\"", false),
        ("\"https://example.com/humidity\"!=\"x\"", false),
        ("\"https://example.com/humidity\"", true),
        ("\"https://example.com/levels\"==5", false),
        ("\"https://example.com/levels\"!=5", false),
        ("\"https://example.com/levels\"", true),
        ("\"https://example.com/route\"==\"urn:x:1\"", false),
        ("\"https://example.com/route\"!=\"urn:x:1\"", false),
        ("\"https://example.com/route\"", true),
        ("\"https://example.com/title\"!=\"Tower\"", false),
        ("\"https://example.com/title\"[en]!=\"Tower\"", false),
        ("\"https://example.com/title\"[en]!~=\"Tow.*\"", false),
        ("\"https://example.com/title\"", true),
        ("\"https://example.com/label\"[de]!=\"Tower\"", true),
    ] {
        let expr = parse(q).expect("the Query Term parses");
        pretty_assertions::assert_eq!(
            filter_holds(&FilterReading::of(&expr, LanguageMatching::TagEquality), &written, NoLinkedEntities),
            expected,
            "{q}"
        );
    }
}

/// A member a fragment deletes (clause 5.5.12) is no member, while a member
/// holding the empty array is one, a defined value that includes no operand
/// (clause 4.9).
#[test]
fn a_deleted_member_is_absent_and_an_empty_member_is_present() {
    let written = document();
    for (q, expected) in [
        ("\"https://example.com/address\"[\"https://example.com/street\"]", false),
        ("\"https://example.com/address\"[\"https://example.com/street\"]!=\"x\"", false),
        ("\"https://example.com/address\"[\"https://example.com/tags\"]", true),
        ("\"https://example.com/address\"[\"https://example.com/tags\"]!=\"x\"", true),
        ("\"https://example.com/address\"[\"https://example.com/tags\"]==\"x\"", false),
    ] {
        let expr = parse(q).expect("the Query Term parses");
        pretty_assertions::assert_eq!(
            filter_holds(&FilterReading::of(&expr, LanguageMatching::TagEquality), &written, NoLinkedEntities),
            expected,
            "{q}"
        );
    }
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
    assert!(geo_query_matches(&near, &written));
    assert!(scope_query_matches(
        &ScopeQuery::parse("/Madrid/#").expect("a scope query"),
        (&written).scopes()
    ));
    assert!(!scope_query_matches(&ScopeQuery::parse("/Berlin").expect("a scope query"), (&written).scopes()));
}
