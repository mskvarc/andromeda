//! An entity on both the broker and an auxiliary context source is retrieved merged, the broker's attributes taking
//! precedence (clauses 4.3.3, 4.3.6.2 and 5.7.1).

use crate::{
    distributed_operations::{
        forwarding_source::{register_in_mode, serve, source_endpoint, stub, type_registration, unregister, vehicle_reply, vehicle_retrieval_rule},
        vehicle_payloads::vehicle_parked2,
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::{Map, Value, json};

/// Pins: Check that if one retrieves entity living on on both the Context Broker and a Context Source, entities get
/// merged correctly
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d010_01_aux_retrieve_entity_that_exists_on_both_the_context_source_and_the_context_broker_from_the_context_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "isParked"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register_in_mode(
        &broker,
        &vocabulary,
        &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?,
        "auxiliary",
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        vehicle_retrieval_rule(&namespace, "", 1),
        StatusCode::OK,
        Some(&vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")),
    )
    .await?;
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .send()
        .await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    expect_that!(body.pointer("/brandName").and_then(Value::as_object).map(Map::len), some(eq(2)));
    expect_that!(body.pointer("/brandName/type"), some(eq(&json!("Property"))));
    expect_that!(body.pointer("/brandName/value"), some(eq(&json!("BMW"))));
    expect_that!(body.pointer("/isParked"), some(anything()));
    expect_that!(body.pointer("/isParked2"), some(anything()));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
