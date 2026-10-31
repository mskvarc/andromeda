//! Every family as a sub attribute reads back as written.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, Lossless, NgsiLdMediaType, Output},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Building:{{ns}}-1",
    "type": "Building_{{ns}}",
    "holder": {
        "type": "Property",
        "value": "holder",
        "speed": {
            "type": "Property",
            "value": 55.5,
            "unitCode": "KMH",
            "valueType": "Speed",
            "observedAt": "2026-01-01T00:00:00.500Z",
            "expiresAt": "2099-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:{{ns}}-a"
        },
        "owner": {
            "type": "Relationship",
            "object": [
                "urn:ngsi-ld:Person:{{ns}}-owner-2",
                "urn:ngsi-ld:Person:{{ns}}-owner-1"
            ],
            "objectType": [
                "Person",
                "Agent"
            ],
            "observedAt": "2026-01-01T00:00:02Z"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    13.5,
                    52.25
                ],
                "bbox": [
                    13.5,
                    52.25,
                    13.5,
                    52.25
                ]
            },
            "observedAt": "2026-01-01T00:00:01Z"
        },
        "name": {
            "type": "LanguageProperty",
            "languageMap": {
                "de": "Bahnhof",
                "en": "Station"
            },
            "valueType": "langString"
        },
        "category": {
            "type": "VocabProperty",
            "vocab": [
                "commercial",
                "office"
            ],
            "valueType": "Category"
        },
        "config": {
            "type": "JsonProperty",
            "json": {
                "a": [
                    1,
                    2.5,
                    {
                        "b": null
                    }
                ],
                "c": "text"
            },
            "valueType": "Config"
        },
        "readings": {
            "type": "ListProperty",
            "valueList": [
                1,
                "two",
                true
            ],
            "unitCode": "CEL",
            "valueType": "Readings"
        },
        "route": {
            "type": "ListRelationship",
            "objectList": [
                {
                    "object": "urn:ngsi-ld:Stop:{{ns}}-route-2"
                },
                {
                    "object": "urn:ngsi-ld:Stop:{{ns}}-route-1"
                }
            ],
            "objectType": [
                "Stop",
                "Place"
            ]
        }
    }
}"#;

/// Pins: A Property carrying one sub-attribute of each of the eight families, every optional member set, reads
/// back member for member as written, system times aside
/// Clause: 5.6.1.4, 5.7.1.4, 4.5.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_0021_every_family_as_a_sub_attribute_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let body = BODY.replace("{{ns}}", namespace.token());
    let building = Entity::<Input>::parse(&body, Lossless::Normalized)?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let expected = Entity::<Output>::parse(&body, Lossless::Normalized)?;
    expect_that!(retrieved.body(), eq(&expected));
    Ok(())
}
