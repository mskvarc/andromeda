//! A downloaded document without a context or not json is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, ErrorType},
};

/// Pins: A create naming a remote document served as application/json that has no top-level @context, or that is
/// not JSON, answers 400 `BadRequestData`
/// Clause: 5.5.6, 5.5.4, 5.5.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0026_a_downloaded_document_without_a_context_or_not_json_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![contexts.reference("jc/no-context.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_18_id = namespace.urn("Vehicle", 18)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let body_context_2 = broker
        .with_context(Context::Multiple(vec![contexts.reference("jc/not-json.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_19_id = namespace.urn("Vehicle", 19)?;
    contexts
        .serve(
            "jc/no-context.jsonld",
            &format!(
                r#"{{
      "speed": "https://test.andromeda/{namespace}/remote/speed"
    }}"#
            ),
            Some("application/json"),
        )
        .await?;
    contexts.serve("jc/not-json.jsonld", "this is not JSON", Some("application/json")).await?;
    let vehicle_18 = Entity::builder()
        .id(vehicle_18_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    let answer = body_context.with_responses().entities().create(&vehicle_18).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_19 = Entity::builder()
        .id(vehicle_19_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    let answer_2 = body_context_2.with_responses().entities().create(&vehicle_19).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
