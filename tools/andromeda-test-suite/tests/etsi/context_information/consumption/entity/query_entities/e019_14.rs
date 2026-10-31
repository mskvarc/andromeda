//! Buildings are queried with their linked City joined inline, with the system attributes of the linked entity (clauses
//! 5.7.2 and 4.5.23).

use crate::linked_entity_fixtures::{create_linked_pairs, delete_linked_pairs};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    query::Join,
};
use serde_json::json;
use std::num::NonZeroU32;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// The pair suffix of the Buildings and Cities of the case.
const PAIR: &str = "019-12";

/// Pins: Check that entities can be queried with an inline linked entity with sysAttrs
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_14_01_query_entities_with_inline_linked_entities_and_sys_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .system_attributes(SystemAttributes::Include)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-12-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-12-1"),
                    "objectType": "City",
                    "entity": {"id": namespace.urn_text("City", "019-12-1"), "type": "City"},
                },
            },
            {
                "id": namespace.urn_text("Building", "019-12-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-12-2"),
                    "objectType": "City",
                    "entity": {"id": namespace.urn_text("City", "019-12-2"), "type": "City"},
                },
            },
        ]))
        .hiding("instanceId")
        .hiding("createdAt")
        .hiding("modifiedAt")
    );
    let linked_created_at = body
        .as_array()
        .into_iter()
        .flatten()
        .map(|building| building.pointer("/locatedAt/entity/createdAt"))
        .collect::<Vec<_>>();
    expect_that!(linked_created_at, each(some(anything())));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
