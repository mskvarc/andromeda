//! A post query bare word without expand values is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST with a q member holding an unquoted bare word is refused with `BadRequestData`,
/// with no `expandValues` member and with one that names another attribute, and the quoted string is answered
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1, 6.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4009_a_post_query_bare_word_without_expand_values_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("status", "active")
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The query language refuses a bare word, which the typed query never writes, so the refused bodies go out as written.
    let fields = [("Content-Type", "application/json"), ("Accept", "application/json")];
    let refused = [
        format!(r#"{{"type": "Query", "entities": [{{"type": "{device_type}"}}], "q": "status==active"}}"#),
        format!(r#"{{"type": "Query", "entities": [{{"type": "{device_type}"}}], "q": "status==active", "expandValues": "level"}}"#),
    ];
    for body in &refused {
        let answer = broker
            .send_unchecked(exchange(&broker, Method::POST, "entityOperations/query", &fields, Some(body.as_str()))?)
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&device_type)
        .q(q::attr("status").eq("active"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&device_id)));
    Ok(())
}
