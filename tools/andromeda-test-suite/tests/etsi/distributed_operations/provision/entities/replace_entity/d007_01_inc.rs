//! Replacing an entity an inclusive registration matches sends the replacement to the context source and replaces the
//! broker's entity as well (clauses 4.3.3, 4.3.6.2 and 5.6.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        vehicle_payloads::vehicle_parked2,
    },
    entity_operations::{create_entity_with_link, delete_entities},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Check that if one requests the Context Broker to replace an entity that matches an inclusive registration,
/// this is replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.18
#[gtest]
#[tokio::test]
async fn d007_01_inc_replace_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "isParked2"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_id_registration(&namespace, 1, 1, &endpoint, Some(RegistrationMode::Inclusive), REDIRECTION)?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let linked = broker.with_context(vocabulary.context())?;
    let reader = linked.with_accept(Accept::any()).with_responses();
    let old = json_body(&reader.entities().retrieve(&id).send().await?)?;
    stub(
        &source,
        ForkRule::resource(Method::PUT, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}"))),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    let replaced = linked.with_responses().entities().replace(&vehicle_parked2(&namespace, 1)?).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    let body = json_body(&reader.entities().retrieve(&id).send().await?)?;
    expect_that!(body, not(eq(&old)));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
