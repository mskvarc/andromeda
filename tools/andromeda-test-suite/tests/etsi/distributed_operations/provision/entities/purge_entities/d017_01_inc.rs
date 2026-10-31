//! Purging the Vehicles an inclusive registration matches purges them on the broker and on the context source (clauses
//! 4.3.3, 4.3.6.2 and 5.6.21).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entities::{
            entity_sources::vehicle_type_registration,
            purge_entities::purge_request::{purge_rule, purge_vehicles},
        },
    },
    entity_operations::create_entity_with_context_string,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Verify that, when one has an inclusive registration on a Context Broker, one is able to purge entities based
/// on type on both Context Broker and Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.21
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d017_01_inc_purge_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_type_registration(&namespace, 1, &endpoint, None)?).await?;
    let source = serve(source).await?;
    stub(&source, purge_rule(&namespace, ""), StatusCode::NO_CONTENT, None).await?;
    purge_vehicles(&broker, &vocabulary).await?;
    expect_that!(stub_count(&source, &purge_rule(&namespace, "")).await?, gt(0));
    let retrieved = broker.with_responses().entities().retrieve(&namespace.urn("Vehicle", 1)?).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await
}
