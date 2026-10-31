//! An entity created on the broker that an inclusive registration matches is created on the context source and on the
//! broker (clauses 4.3.3, 4.3.6.2 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        redirected_request::{forwarded_body, wait_for_request},
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, query::Locality};
use serde_json::{Value, json};

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration,
/// this is created on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.1
/// Expected: the registration names the redirection operations, so the create is forwarded (Table 4.20-2)
#[gtest]
#[tokio::test]
async fn d001_01_inc_create_entity_on_both_context_broker_and_context_source() -> Result<()> {
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
        StatusCode::CREATED,
        None,
    )
    .await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let forwarded = forwarded_body(&wait_for_request(&source, 0).await?)?;
    let id = namespace.urn_text("Vehicle", 1);
    expect_that!(forwarded.get("id"), some(eq(&json!(id))));
    let listed = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&listed, answered(StatusCode::OK))?;
    let body = json_body(&listed?)?;
    let entities = body.as_array().or_fail()?;
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
