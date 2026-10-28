//! ETSI `019_25`: every entity of a tenant is queried with the `local` flag alone, without an entity type or a
//! selection (clause 5.7.2).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::Locality};
use serde_json::json;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 1] = ["Building"];

/// Pins: Check that one can query all entities if query is local
/// Clause: 5.7.2
/// Tenant: private (a query with local=true alone answers the first page of every entity of the tenant, which in the
/// shared tenant need not hold this file's two entities)
/// Changed: the response is compared as one list of the two Buildings, where the Hurl file checks each Building's `id`
/// and `type` and that it has two members
#[gtest]
#[tokio::test]
async fn e019_25_query_all_entities_with_local_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    create_entity(&tenant, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&tenant, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let received = tenant
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", 1),
                "type": namespace.expanded_term("Building")?,
            },
            {
                "id": namespace.urn_text("Building", 2),
                "type": namespace.expanded_term("Building")?,
            },
        ]))
    );
    delete_entities(&tenant, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
