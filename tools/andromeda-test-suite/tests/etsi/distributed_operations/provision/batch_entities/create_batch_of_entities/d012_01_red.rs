//! A batch create of entities two redirect registrations match is sent to both context sources once (clauses 4.3.3,
//! 4.3.6.3 and 5.6.7).

use crate::{
    batch_operations::batch_create,
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::batch_sources::{batch_registration, delete_vehicles},
        vehicle_payloads::vehicle_speed_parked2,
    },
    entity_operations::ld_json_body_client,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that if one requests the Context Broker to create a batch of entities that match a redirect
/// registration, these are created on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.7
/// Isolation: the registration's idPattern is `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that names no entity type
/// matches a registration on the id alone (clause 5.12), so a pattern without the namespace would draw every other
/// test's requests on a Vehicle id into this test's source
#[gtest]
#[tokio::test]
async fn d012_01_red_batch_create_entities_with_redirect_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed", "source", "isParked2", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &batch_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect))?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entityoperations/create")));
    stub(&source, rule("/broker1"), StatusCode::CREATED, None).await?;
    stub(&source, rule("/broker2"), StatusCode::CREATED, None).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    batch_create(&writer, &[vehicle_speed_parked2(&namespace, 1)?, vehicle_speed_parked2(&namespace, 2)?]).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
