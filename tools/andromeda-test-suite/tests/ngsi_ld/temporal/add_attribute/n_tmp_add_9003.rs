//! Adding or modifying a temporal instance refuses the null under a key.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    value::object,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, JsonProperty, NgsiLdMediaType, Property, RawJson, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Add Attributes to Temporal Evolution and Modify Attribute instance each answer 400 `BadRequestData` for a
/// Property value, JSON literal or `JsonProperty` holding urn:ngsi-ld:null under a key, and the evolution keeps its
/// one instance
/// Clause: 5.5.4, 5.6.12.4, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_9003_adding_or_modifying_a_temporal_instance_refuses_the_null_under_a_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_temporal_existing_id = namespace.urn("LiteralProbe", "temporal-existing")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let core = config.core_context();
    let literal_probe_temporal_existing_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_existing_id.clone())
        .entity_type(literal_probe_type.clone())
        .property(
            "config",
            Property::builder(object([("settings", object([("a", Value::from(1))]))])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    body_context
        .temporal()
        .create_or_update(&literal_probe_temporal_existing_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_existing_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance = instance_id_at(evolution, &"config".parse()?, 0)?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"street\": \"x\",\n                \"country\": \"urn:ngsi-ld:null\"\n            }},\n            \"observedAt\": \"2024-01-01T00:00:00Z\"\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_2 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs/config/{instance}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"street\": \"x\",\n                \"country\": \"urn:ngsi-ld:null\"\n            }}\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this temporal fragment, so the body is sent as written.
    let request_3 = request_to(&broker, Method::POST, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"settings\": {{\n                    \"a\": 1,\n                    \"b\": \"urn:ngsi-ld:null\"\n                }}\n            }},\n            \"observedAt\": \"2024-01-01T00:00:00Z\"\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_4 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs/config/{instance}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"settings\": {{\n                    \"a\": 1,\n                    \"b\": \"urn:ngsi-ld:null\"\n                }}\n            }}\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this temporal fragment, so the body is sent as written.
    let request_5 = request_to(&broker, Method::POST, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"@type\": \"@json\",\n                \"@value\": {{\n                    \"a\": 1,\n                    \"b\": \"urn:ngsi-ld:null\"\n                }}\n            }},\n            \"observedAt\": \"2024-01-01T00:00:00Z\"\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_6 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs/config/{instance}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": {{\n                \"@type\": \"@json\",\n                \"@value\": {{\n                    \"a\": 1,\n                    \"b\": \"urn:ngsi-ld:null\"\n                }}\n            }}\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let instances = TemporalEntity::builder()
        .id(literal_probe_temporal_existing_id.clone())
        .entity_type(literal_probe_type.clone())
        .json_property(
            "config",
            JsonProperty::builder(r#"{"a":1,"b":"urn:ngsi-ld:null"}"#.parse::<RawJson>()?).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?
        .attributes;
    let answer_7 = body_context
        .with_responses()
        .temporal()
        .add_attributes(&literal_probe_temporal_existing_id, &instances)
        .send()
        .await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_7 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{literal_probe_temporal_existing_id}/attrs/config/{instance}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"config\": [\n        {{\n            \"type\": \"JsonProperty\",\n            \"json\": {{\n                \"a\": 1,\n                \"b\": \"urn:ngsi-ld:null\"\n            }}\n        }}\n    ],\n    \"@context\": [\n        {{\n            \"settings\": {{\n                \"@id\": \"https://example.org/settings\",\n                \"@type\": \"@json\"\n            }}\n        }},\n        \"{core}\"\n    ]\n}}")));
    let answer_8 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_existing_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "config",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .instance_id(&instance)
                        .value_member(["https://example.org/settings", "@value", "a"], 1)
                )
            )
        )
    );
    Ok(())
}
