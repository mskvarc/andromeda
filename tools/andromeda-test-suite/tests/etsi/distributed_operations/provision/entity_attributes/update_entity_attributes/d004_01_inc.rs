//! An update of an entity only an inclusive registration holds is forwarded to the context source with the written
//! `brandname` (clauses 4.3.3, 4.3.6.2 and 5.6.2).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{vehicle_attribute_registration, vehicle_rule},
        redirected_request::{forwarded_body, wait_for_redirected},
    },
    entity_operations::update_attributes,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::ContextDelivery, model::EntityFragment};
use serde_json::json;

/// Pins: Check that if one request the Context Broker to update an entity that matches an inclusive registration, this
/// is updated on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.2
/// Expected: the update of a remote-only entity answers 204, not 207 (clause 5.6.2.4)
#[gtest]
#[tokio::test]
async fn d004_01_inc_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandname"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_attribute_registration(&namespace, 1, &endpoint, None, &[])?).await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::PATCH, "", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let fragment = EntityFragment::builder().property("brandname", "BMW").build()?;
    update_attributes(&writer, &namespace.urn("Vehicle", 1)?, &fragment).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    expect_that!(forwarded_body(&forwarded)?.pointer("/brandname/value"), some(eq(&json!("BMW"))));
    unregister(&broker, &namespace, 1).await
}
