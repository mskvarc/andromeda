//! Post query with pick or omit and no narrowing input is too wide.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: a Query body whose only restriction is a pick or an omit (with a `scopeQ`, a lang or a `datasetId` beside
/// it at most) is too wide and refused with `BadRequestData`, and the same pick beside an entity selector is
/// answered
/// Clause: 5.7.2.4, 5.2.23, 6.23.3.1, 4.21
#[gtest]
#[tokio::test]
async fn n_qry_ent_3031_post_query_with_pick_or_omit_and_no_narrowing_input_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("p", 1)
        .scope("/Madrid")
        .build()?;
    broker.entities().batch_create(&[building]).send().await?.done()?;
    // The client refuses a projection with nothing narrowing it, so each body goes out as written.
    let json = [("Content-Type", "application/json"), ("Accept", "application/json")];
    let ld_json = [("Content-Type", "application/ld+json"), ("Accept", "application/json")];
    let linked = format!(r#"{{"type": "Query", "lang": "en", "pick": ["p"], "@context": "{}"}}"#, config.core_context());
    let calls = [
        (&json, r#"{"type": "Query", "pick": ["p"]}"#.to_owned()),
        (&json, r#"{"type": "Query", "omit": ["p"]}"#.to_owned()),
        (&json, r#"{"type": "Query", "scopeQ": "/Madrid", "pick": ["id", "p"]}"#.to_owned()),
        (&ld_json, linked),
    ];
    for (fields, body) in &calls {
        let request = exchange(&broker, Method::POST, "entityOperations/query", *fields, Some(body.as_str()))?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .pick("id,p".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.id.as_ref(), some(eq(&building_id)));
    expect_that!(item, has_attribute("p", property().value(1)));
    Ok(())
}
