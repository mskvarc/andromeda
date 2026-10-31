//! An append of attributes to an entity two redirect registrations match is sent to both context sources once and
//! creates no entity on the broker (clauses 4.3.3, 4.3.6.3 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{parked_by_john_doe_and_speed, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::append_attributes,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    query::Locality,
    registration::RegistrationMode,
};

/// Pins: Check that an entity attribute is appended and the redirect registration forwards the request to the Context
/// Sources
/// Clause: 4.3.3, 4.3.6.3, 5.6.3
/// Expected: providedBy stays out of the file vocabulary: the test reads it without a @context, so it expands under the
/// core default vocabulary
#[gtest]
#[tokio::test]
async fn d003_01_red_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "isParked", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &vehicle_attribute_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect), &[])?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| vehicle_rule(&namespace, Method::POST, below, "/attrs");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    append_attributes(&broker, &id, &parked_by_john_doe_and_speed(&namespace)?, Overwrite::Replace).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
