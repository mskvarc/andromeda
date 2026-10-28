//! ETSI `D001_04_inc`: an entity created with `local=true` that an inclusive registration matches is not forwarded to
//! the context source (clauses 4.3.3, 4.3.6.2, 5.6.1 and 6.3.18).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    suite_keywords::delete_entities,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::sleep;

/// How long the case waits for a request that must not reach the source.
const ABSENCE_WINDOW: Duration = Duration::from_secs(5);

/// Pins: Verify that, when one has an inclusive registration on a Context Broker, one is able to create an entity
/// with local flag and the operation should not be forwarded to the Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.1, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
/// Absence: Wait For No Request 5 s
#[gtest]
#[tokio::test]
async fn d001_04_inc_create_entity_with_local_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    sleep(ABSENCE_WINDOW).await;
    expect_that!(source.received().await?, is_empty());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
