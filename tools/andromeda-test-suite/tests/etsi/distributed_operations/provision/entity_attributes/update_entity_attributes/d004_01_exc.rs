//! ETSI `D004_01_exc`: an update of the `speed` an exclusive registration holds is forwarded to the context source
//! (clauses 4.3.3, 4.3.6.3 and 5.6.2).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{vehicle_attribute_registration, vehicle_rule},
    },
    suite_keywords::{delete_entities, ld_json_body_client, update_attributes},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{EntityFragment, Property},
    registration::RegistrationMode,
};

/// Pins: Check that the entity is updated correctly in the Context Source via redirectionOps
/// Clause: 4.3.3, 4.3.6.3, 5.6.2
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d004_01_exc_create_entity_and_registration_and_start_context_source_mock_server() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Exclusive), &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::PATCH, "/broker1", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .entity_type("Vehicle")
        .property("speed", Property::builder(66).property("source", "Speedometer"))
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
