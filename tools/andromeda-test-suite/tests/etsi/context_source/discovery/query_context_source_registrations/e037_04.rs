//! A query of context source registrations sent without a JSON-LD context resolves its terms against the core context
//! and answers the registration with its terms expanded (clause 5.10.2).

use crate::{
    registration_operations::{create_registration, csource_endpoint, delete_registrations},
    registration_payloads::building_registration,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that one can query context source registrations. If a JSON-LD context is not provided, then all the
/// query terms shall be resolved against the default JSON-LD @context
/// Clause: 5.10.2
/// Expected: locatedAt stays out of the file vocabulary: the test reads it without a @context, so it expands under the
/// core default vocabulary
#[gtest]
#[tokio::test]
async fn e037_04_01_query_context_source_registrations_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory"]).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let _registration = create_registration(&broker, &vocabulary, &building_registration(&namespace, 1, &endpoint)?).await?;

    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .registrations()
        .query()
        .ids(vec![namespace.urn("ContextSourceRegistration", 1)?])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 1),
            "type": "ContextSourceRegistration",
            "information": [{
                "entities": [{"type": namespace.expanded_term("Building")?}],
                "propertyNames": [namespace.expanded_term("name")?, namespace.expanded_term("subCategory")?],
                "relationshipNames": ["locatedAt"]
            }],
            "endpoint": endpoint.as_str(),
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1]).await
}
