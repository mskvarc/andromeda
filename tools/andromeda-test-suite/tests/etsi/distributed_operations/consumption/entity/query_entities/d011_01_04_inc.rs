//! A query by type and an attribute neither the broker's entity nor the inclusive context source's holds answers no
//! entity (clauses 4.3.3, 4.3.6.2 and 5.7.2).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, type_registration, unregister},
        vehicle_payloads::vehicle_parked2,
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::AttributeName};
use serde_json::json;

/// Pins: Check that if one queries for attribute present in neither of the entities, neither of them gets returned.
/// Contrast with `D011_01_02`, where no local entity exists at all.
/// Clause: 4.3.3, 4.3.6.2, 5.7.2
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_01_04_inc_query_the_context_broker_with_type_and_attribute_in_neither() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/ngsi-ld/v1/entities"),
            &["attrs", "speed", "type", "vehicle"],
            Some(&[]),
        ),
        StatusCode::OK,
        Some(&json!([])),
    )
    .await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .attrs(vec!["speed".parse::<AttributeName>()?])
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_array(), some(is_empty()));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
