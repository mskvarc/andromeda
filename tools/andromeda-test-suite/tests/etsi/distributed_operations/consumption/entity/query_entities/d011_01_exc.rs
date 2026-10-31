//! A query by type asks the exclusive context source for the attribute it registers and merges it into the broker's
//! entity (clauses 4.3.3, 4.3.6.3 and 5.7.2).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister, vehicle_registration},
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};
use serde_json::json;

/// Pins: Check that if one queries the Context Broker for type, entity with matching type on a Context Source gets
/// merged correctly
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
#[gtest]
#[tokio::test]
async fn d011_01_exc_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    let vehicle = namespace.urn("Vehicle", 1)?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let registration = vehicle_registration(
        &namespace,
        1,
        &source_endpoint(&source, &namespace, "/broker1")?,
        RegistrationMode::Exclusive,
        &["speed"],
    )?;
    let _registration = register(&broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    let rule = || {
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"),
            &["type", "vehicle", "id", &namespace.urn_text("Vehicle", 1), "attrs", "speed"],
            Some(&["id", "speed", "type"]),
        )
    };
    let reply = json!([{
        "id": namespace.urn_text("Vehicle", 1),
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 56, "source": {"type": "Property", "value": "Speedometer"}},
    }]);
    stub(&source, rule(), StatusCode::OK, Some(&reply)).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.pointer("/0/speed"), some(anything()));
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&vehicle]).await;
    Ok(())
}
