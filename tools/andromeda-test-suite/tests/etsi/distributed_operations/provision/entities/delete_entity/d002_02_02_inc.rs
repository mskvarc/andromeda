//! Deleting an entity that exists on the context source alone, which an inclusive registration names, is answered 204
//! (clauses 4.3.3, 4.3.6.2, 5.6.6 and 6.3.3).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
    provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Verify that, when one has an inclusive registration on a Context Broker, one is able to delete entities on
/// a Context Source and should get a `BatchOperationResult` structure
/// Clause: 4.3.3, 4.3.6.2, 5.6.6, 6.3.3
/// Expected: the delete of a remote-only entity answers 204, not 207 (clause 5.6.6.4)
#[gtest]
#[tokio::test]
async fn d002_02_02_inc_delete_entity_on_a_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    stub(
        &source,
        ForkRule::resource(Method::DELETE, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}"))),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    expect_that!(&broker.with_responses().entities().delete(&id).send().await, answered(StatusCode::NO_CONTENT));
    broker.temporal().delete(&id).send().await.any_answer();
    unregister(&broker, &namespace, 1).await
}
