//! Each item of a batch is expanded under its own inline context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Batch Entity Creation and Batch Entity Upsert as application/ld+json whose items name speed through
/// different inline @contexts store each item under its own IRI and never under another item's
/// Clause: 6.3.5, 5.6.7, 5.6.8, JSON-LD 1.1 s3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0028_each_item_of_a_batch_is_expanded_under_its_own_inline_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let item = |suffix: &str, context: &str| {
        format!(
            r#"{{
        "id": "urn:ngsi-ld:Vehicle:{{{{ns}}}}-{suffix}",
        "type": "Vehicle_{{{{ns}}}}",
        "speed": {{
            "type": "Property",
            "value": 1
        }},
        "@context": [
            {{
                "speed": "https://test.andromeda/{{{{ns}}}}/{context}/speed"
            }},
            "{{{{core_context}}}}"
        ]
    }}"#
        )
    };
    let create_body = format!("[\n    {},\n    {},\n    {}\n]", item("1", "a"), item("2", "b"), item("3", "a"))
        .replace("{{core_context}}", core.as_str())
        .replace("{{ns}}", &namespace.to_string());
    // The model writes one @context into the whole batch body, so a batch whose items each carry their own is sent
    // as written.
    let request = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(create_body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let a_speed = format!("https://test.andromeda/{namespace}/a/speed");
    let b_speed = format!("https://test.andromeda/{namespace}/b/speed");
    for (suffix, speed) in [("1", &a_speed), ("2", &b_speed), ("3", &a_speed)] {
        let vehicle_id = namespace.urn("Vehicle", suffix)?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await?;
        let entity = retrieved.body();
        expect_that!(entity, has_attribute(speed, any_attribute().value(1)));
        expect_that!(entity, lacks_attribute(if speed == &a_speed { &b_speed } else { &a_speed }));
    }
    let upsert_body = format!("[\n    {},\n    {},\n    {}\n]", item("4", "a"), item("5", "b"), item("6", "a"))
        .replace("{{core_context}}", core.as_str())
        .replace("{{ns}}", &namespace.to_string());
    let request_2 = request_to(&broker, Method::POST, "entityOperations/upsert")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(upsert_body));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    for (suffix, speed) in [("4", &a_speed), ("5", &b_speed), ("6", &a_speed)] {
        let vehicle_id = namespace.urn("Vehicle", suffix)?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await?;
        let entity = retrieved.body();
        expect_that!(entity, has_attribute(speed, any_attribute().value(1)));
        expect_that!(entity, lacks_attribute(if speed == &a_speed { &b_speed } else { &a_speed }));
    }
    Ok(())
}
