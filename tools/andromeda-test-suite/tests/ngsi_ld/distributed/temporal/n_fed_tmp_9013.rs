//! A scope beside the modified instance is refused and not forwarded.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, source_answer, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    model::ErrorType,
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Modify Attribute instance with a scope history beside the speed instance, under an inclusive registration
/// for `updateAttrs`, answers 400 `BadRequestData` and forwards nothing
/// Clause: 5.6.14.3, 5.6.14.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9013_a_scope_beside_the_modified_instance_is_refused_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let reified_scope_forwarding_instance_id = namespace.urn("ReifiedScopeForwarding", "instance")?;
    let instance_scope_id = namespace.urn("Instance", "scope")?;
    let registration_id = namespace.registration_id(1)?;
    let reified_scope_forwarding_type = namespace.entity_type("ReifiedScopeForwarding")?;
    source
        .answer(
            Method::PATCH,
            &format!("/scope/ngsi-ld/v1/temporal/entities/{reified_scope_forwarding_instance_id}/attrs/speed/{instance_scope_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("scope")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(reified_scope_forwarding_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpdateAttrs])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("temporal/entities/{reified_scope_forwarding_instance_id}/attrs/speed/{instance_scope_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"scope\": [\n        {\n            \"type\": \"Property\",\n            \"value\": \"/Madrid/Gardens\",\n            \"observedAt\": \"2025-01-01T00:00:00Z\",\n            \"createdAt\": \"2025-01-01T00:00:01Z\",\n            \"modifiedAt\": \"2025-01-01T00:00:02Z\"\n        },\n        {\n            \"type\": \"Property\",\n            \"value\": \"urn:ngsi-ld:null\",\n            \"observedAt\": \"2025-01-02T00:00:00Z\",\n            \"deletedAt\": \"2025-01-02T00:00:00Z\"\n        }\n    ],\n    \"speed\": [\n        {\n            \"type\": \"Property\",\n            \"value\": 10,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }\n    ]\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let forwarded = source
        .received_on(
            Method::PATCH,
            &format!("/scope/ngsi-ld/v1/temporal/entities/{reified_scope_forwarding_instance_id}/attrs/speed/{instance_scope_id}"),
        )
        .await?;
    expect_that!(forwarded, len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
