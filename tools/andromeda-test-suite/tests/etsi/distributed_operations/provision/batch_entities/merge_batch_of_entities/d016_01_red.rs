//! A batch merge of two entities two redirect registrations match is sent to both context sources once, and a later
//! query reads the merged `speed` from the sources (clauses 4.3.3, 4.3.6.3 and 5.6.20).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
    provision::batch_entities::{
        batch_sources::{batch_registration, delete_vehicles},
        merge_batch_of_entities::merge_request::{merge_vehicles, merged_vehicles_reply},
    },
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{model::NgsiLdMediaType, registration::RegistrationMode};

/// The attributes a query of the sources may name in its `attrs`.
const QUERY_ATTRS: [&str; 5] = ["@context", "id", "isParked2", "speed", "type"];

/// Pins: Check that if one requests the Context Broker to merge a batch of entities that match an exclusive
/// registration, they are merged on the Context Source too.
/// Clause: 4.3.3, 4.3.6.3, 5.6.20
/// Expected: the query that follows the merge is forwarded to the source, which answers it (clause 4.3.6.3)
/// Isolation: the registration's idPattern is `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that names no entity type
/// matches a registration on the id alone (clause 5.12), so a pattern without the namespace would draw every other
/// test's requests on a Vehicle id into this test's source
#[gtest]
#[tokio::test]
async fn d016_01_red_merge_batch_entities_on_the_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
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
    let operation_rule = |below: &str, operation: &str| {
        ForkRule::resource(
            Method::POST,
            &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entityoperations/{operation}")),
        )
    };
    stub(&source, operation_rule("/broker1", "create"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, operation_rule("/broker2", "create"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, operation_rule("/broker1", "merge"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, operation_rule("/broker2", "merge"), StatusCode::NO_CONTENT, None).await?;
    merge_vehicles(&broker, &vocabulary, &namespace).await?;
    expect_that!(stub_count(&source, &operation_rule("/broker1", "merge")).await?, eq(1));
    expect_that!(stub_count(&source, &operation_rule("/broker2", "merge")).await?, eq(1));
    let reply = merged_vehicles_reply(&namespace, &vocabulary);
    for below in ["/broker1", "/broker2"] {
        let rule = ForkRule::query(
            Method::GET,
            &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities")),
            &["type", "vehicle"],
            Some(&QUERY_ATTRS),
        );
        stub(&source, rule, StatusCode::OK, Some(&reply)).await?;
    }
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .send()
        .await?;
    expect_that!(json_body(&received)?.pointer("/0/speed"), some(anything()));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
