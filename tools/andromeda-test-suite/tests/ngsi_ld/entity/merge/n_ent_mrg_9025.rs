//! A merge and a partial attribute update under user aliases update the value.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Merge Entity and Partial Attribute Update under an inline @context aliasing @type as kind and
/// `ngsi-ld:hasValue` as v answer 204 and write the value each names
/// Clause: 4.4, 5.5.7, 5.6.4, 5.6.17
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9025_a_merge_and_a_partial_attribute_update_under_user_aliases_update_the_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merged_id = namespace.urn("Building", "merged")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merged = Entity::builder()
        .id(building_merged_id.clone())
        .entity_type(building_type)
        .property("height", 1)
        .build()?;
    broker.entities().create(&building_merged).send().await?.done()?;
    // The aliases are the point: the keywords `kind` and `v` stand for `type` and `hasValue` only through the
    // body's own @context, which the model does not write for a fragment.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{building_merged_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from_static(
            b"{\"height\": {\"kind\": \"Property\", \"v\": 2}, \"@context\": {\"kind\": \"@type\", \"v\": \"https://uri.etsi.org/ngsi-ld/hasValue\"}}",
        ));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merged_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("height", property().value(2)));
    let request_2 = request_to(&broker, Method::PATCH, &format!("entities/{building_merged_id}/attrs/height"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from_static(
            b"{\"kind\": \"Property\", \"v\": 3, \"@context\": {\"kind\": \"@type\", \"v\": \"https://uri.etsi.org/ngsi-ld/hasValue\"}}",
        ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merged_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("height", property().value(3).without_sub_attribute("v")));
    Ok(())
}
