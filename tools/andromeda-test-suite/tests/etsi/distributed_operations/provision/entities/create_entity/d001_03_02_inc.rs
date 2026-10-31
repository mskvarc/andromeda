//! An entity that exists on the broker and on the context source an inclusive registration names is refused by both,
//! answered 409 `AlreadyExists` (clauses 4.3.3, 4.3.6.2, 5.6.1 and 6.3.3).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    entity_operations::{create_entity_with_context_string, delete_entities, send_create_entity},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::ContextDelivery, model::ErrorType};

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration and
/// already exists both locally and remotely, this raises an error on both Context Broker and Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.1, 6.3.3
/// Expected: a distributed create that fails completely answers 409, not 207 (clause 5.6.1.4)
#[gtest]
#[tokio::test]
async fn d001_03_02_inc_create_entity_already_existing_on_both_context_broker_and_context_source() -> Result<()> {
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
        StatusCode::CONFLICT,
        None,
    )
    .await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_create_entity(&writer, &vehicle_parked(&namespace, 1)?).await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
