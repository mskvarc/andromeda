//! Deleting two entities, each matched by its own redirect registration, deletes each on the context source of its
//! registration (clauses 4.3.3, 4.3.6.3 and 5.6.6).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    entity_operations::create_entity_with_context_string,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has a redirect registration on a Context Broker, one is able to delete entities on
/// both Context Sources
/// Clause: 4.3.3, 4.3.6.3, 5.6.6
#[gtest]
#[tokio::test]
async fn d002_01_red_delete_entities_on_both_context_sources() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (n, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        let registration = vehicle_id_registration(&namespace, n, n, &endpoint, Some(RegistrationMode::Redirect), REDIRECTION)?;
        registrations.push(register(&broker, &vocabulary, &registration).await?);
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let creation = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities")));
    stub(&source, creation("/broker1"), StatusCode::CREATED, None).await?;
    stub(&source, creation("/broker2"), StatusCode::CREATED, None).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let first = namespace.urn("Vehicle", 1)?;
    let second = namespace.urn("Vehicle", 2)?;
    let deletion = |below: &str, n: u32| {
        ForkRule::resource(
            Method::DELETE,
            &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities/{}", namespace.urn_text("Vehicle", n))),
        )
    };
    stub(&source, deletion("/broker1", 1), StatusCode::NO_CONTENT, None).await?;
    stub(&source, deletion("/broker2", 2), StatusCode::NO_CONTENT, None).await?;
    expect_that!(
        &broker.with_responses().entities().delete(&first).send().await,
        answered(StatusCode::NO_CONTENT)
    );
    broker.temporal().delete(&first).send().await.any_answer();
    expect_that!(
        &broker.with_responses().entities().delete(&second).send().await,
        answered(StatusCode::NO_CONTENT)
    );
    broker.temporal().delete(&second).send().await.any_answer();
    expect_that!(stub_count(&source, &deletion("/broker1", 1)).await?, gt(0));
    expect_that!(stub_count(&source, &deletion("/broker2", 2)).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
