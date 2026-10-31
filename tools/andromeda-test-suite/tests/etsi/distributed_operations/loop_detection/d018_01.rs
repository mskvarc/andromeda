//! A request whose `Via` header already names the broker, as the broker's own forwarded request does, is refused as a
//! loop (clauses 4.3.3, 5.6.6 and 6.3.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister, vehicle_registration},
        loop_detection::via_header::forwarded_via,
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::VIA};
use ngsi_ld::{http::operation::DeleteEntity, registration::RegistrationMode};

/// Pins: Verify that a loop is detected when the Via header contains the broker's identifier.
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Expected: the registration is a redirect one, so the Via loop back answers 508 (clause 6.3.17)
#[gtest]
#[tokio::test]
async fn d018_01_loop_detection_with_via_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_registration(&namespace, 1, &endpoint, RegistrationMode::Redirect, &[])?,
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entities")),
        StatusCode::CREATED,
        None,
    )
    .await?;
    let vehicle = namespace.urn("Vehicle", 1)?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let via = forwarded_via(&source).await?.or_fail()?;
    let request = broker
        .unchecked(&DeleteEntity {
            id: &vehicle,
            entity_type: None,
        })?
        .append_header(VIA, HeaderValue::from_str(&via)?);
    let deleted = broker.send_unchecked(request).await;
    expect_that!(&deleted, answered(StatusCode::LOOP_DETECTED));
    broker.temporal().delete(&vehicle).send().await.any_answer();
    delete_entities(&broker, &[&vehicle]).await;
    unregister(&broker, &namespace, 1).await
}
