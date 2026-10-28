//! ETSI `D014_01_exc`: a batch update of two broker entities, whose `speed` an exclusive registration holds, is sent
//! to the context source (clauses 4.3.3, 4.3.6.3 and 5.6.9).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::{delete_vehicles, exclusive_batch_registration},
            update_batch_of_entities::update_request::update_vehicles,
        },
    },
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::operation::Overwrite;

/// Pins: Check that if one request the Context Broker to update a batch of entities that match a redirect
/// registration, they are updated on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.9
/// Corrected: `fix(D014_15_exc)`: exclusive batch by id with no local covered attribute (4.3.6.3, 5.9.2.4)
/// Corrected: `fix(D012_01_exc)`: exclusive batch registration names entities by id (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d014_01_exc_update_batch_entities_without_no_overwrite_option() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &exclusive_batch_registration(&namespace, &source_endpoint(&source, &namespace, "/broker1")?)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/update"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    update_vehicles(&broker, &vocabulary, &namespace, Overwrite::Replace).await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
