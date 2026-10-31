//! An entity that lives on an inclusive context source alone is retrieved through the broker (clauses 4.3.3, 4.3.6.2
//! and 5.7.1).

use crate::distributed_operations::forwarding_source::{
    register,
    serve,
    source_endpoint,
    stub,
    type_registration,
    unregister,
    vehicle_reply,
    vehicle_retrieval_rule,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that if one retrieves entity living on a Context Source from a Context Broker, entity gets returned
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d010_01_inc_retrieve_entity_on_a_context_source_from_the_context_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    stub(
        &source,
        vehicle_retrieval_rule(&namespace, "", 1),
        StatusCode::OK,
        Some(&vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")),
    )
    .await?;
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::OK));
    unregister(&broker, &namespace, 1).await
}
