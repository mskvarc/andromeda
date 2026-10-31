//! An entity that exists on the context source an inclusive registration names is refused by the source and created on
//! the broker, answered 207 (clauses 4.3.3, 4.3.6.2, 5.6.1 and 6.3.3).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    entity_operations::{delete_entities, send_create_entity},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    query::Locality,
};
use serde_json::{Value, json};

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration and
/// already exists remotely, this raises an error on the Context Source, but it works just fine on the Context Broker
/// Clause: 4.3.3, 4.3.6.2, 5.6.1, 6.3.3
#[gtest]
#[tokio::test]
async fn d001_03_03_inc_create_entity_already_existing_remotely_on_the_context_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entities")),
        StatusCode::CONFLICT,
        None,
    )
    .await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_create_entity(&writer, &vehicle_parked(&namespace, 1)?).await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let body = serde_json::from_slice::<Value>(answer?.body())?;
    expect_that!(body.pointer("/errors/0/error/status"), some(any![eq(&json!(409)), eq(&json!("409"))]));
    let listed = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await?;
    let body = json_body(&listed)?;
    let entities = body.as_array().or_fail()?;
    let id = namespace.urn_text("Vehicle", 1);
    expect_that!(entities, len(eq(1)));
    expect_that!(
        entities
            .iter()
            .find(|entity| entity.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(Value::as_object),
        some(anything())
    );
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
