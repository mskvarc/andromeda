//! An entity that exists on the broker and that an inclusive registration matches is refused by the broker and created
//! on the context source, answered 207 (clauses 4.3.3, 4.3.6.2, 5.6.1 and 6.3.3).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        redirected_request::{forwarded_body, wait_for_redirected},
    },
    entity_operations::{create_entity_with_context_string, delete_entities, send_create_entity},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::ContextDelivery;
use serde_json::{Value, json};

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration and
/// already exists locally, this raises an error on the Context Broker but is created correctly on the Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.1, 6.3.3
#[gtest]
#[tokio::test]
async fn d001_03_01_inc_create_entity_already_existing_locally_on_a_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entities")),
        StatusCode::CREATED,
        None,
    )
    .await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_create_entity(&writer, &vehicle_parked(&namespace, 1)?).await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let body = serde_json::from_slice::<Value>(answer?.body())?;
    expect_that!(body.pointer("/errors/0/error/status"), some(any![eq(&json!(409)), eq(&json!("409"))]));
    expect_that!(body.get("errors").and_then(Value::as_array), some(len(eq(1))));
    let forwarded = forwarded_body(&wait_for_redirected(&source, 0).await?)?;
    expect_that!(forwarded.get("id"), some(eq(&json!(namespace.urn_text("Vehicle", 1)))));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
