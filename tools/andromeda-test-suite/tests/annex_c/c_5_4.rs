//! Annex C.5.4: a query limited to two of three Vehicles answers two and a `next` pointer whose page holds the third
//! (clauses 5.5.9 and 6.3.10).

use crate::{
    brand_fleet::{BRANDS, create_brand_fleet, simplified_brand_vehicle},
    served_context::{ServedContexts, linked},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    wire::{self, header_values},
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, LINK},
};
use ngsi_ld::{model::NgsiLdMediaType, query::SelectorType};
use regex::Regex;
use serde_json::Value;
use url::Url;

/// `item` without its `@context` member, as a page item may carry one or not.
fn without_context(item: &Value) -> Value {
    // The page stays as received; the copy is what is compared.
    let mut item = item.clone();
    if let Some(members) = item.as_object_mut() {
        members.remove("@context");
    }
    item
}

/// Pins: Query Entities with limit=2 over three Vehicles answers two simplified Vehicles and a next pointer whose
/// page holds the third alone, whichever pointer form 5.5.9 lets the broker choose
/// Clause: C.5.4, 5.5.9, 5.7.2, 6.3.10
/// Expected: the Vehicles hold only brandName, the one Attribute the printed answer shows for a request without attrs
/// (clause 5.7.2)
/// Expected: a third Vehicle exists for the next pointer to lead to, and the page members are not named (clause 5.7.2)
#[gtest]
#[tokio::test]
async fn c_5_4_0001_the_first_page_of_two_points_to_the_third_vehicle() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    create_brand_fleet(&broker, &namespace).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type(namespace.suffixed("Vehicle").parse::<SelectorType>()?)
        .limit(2)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let pointer = Regex::new(r#"<([^>]*)>; rel="next""#)?;
    let next = header_values(received.head.headers(), &LINK)
        .into_iter()
        .find_map(|value| {
            pointer
                .captures(value)
                .and_then(|captures| captures.get(1))
                .map(|found| found.as_str().to_owned())
        })
        .or_fail()?;
    let page: Value = serde_json::from_slice(&received.body)?;
    let items = page.as_array().or_fail()?;
    let printed = BRANDS.map(|(suffix, brand)| simplified_brand_vehicle(&namespace, suffix, brand));
    expect_that!(items.len(), eq(2));
    for item in items {
        expect_that!(without_context(item), any![eq(&printed[0]), eq(&printed[1]), eq(&printed[2])]);
    }
    let first: Vec<&Value> = items.iter().filter_map(|item| item.get("id")).collect();
    let target = Url::parse(broker.endpoint().base_url().as_str())?.join(&next)?;
    let request = wire::request_to(&broker, Method::GET, "entities")?
        .uri(target.as_str().parse()?)
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    let second: Value = serde_json::from_slice(answer?.body())?;
    let second = second.as_array().or_fail()?;
    expect_that!(second.len(), eq(1));
    let third = second.first().and_then(|item| item.get("id")).or_fail()?;
    expect_that!(first, not(contains(eq(&third))));
    expect_that!(
        third.as_str(),
        some(matches_regex(format!("^urn:ngsi-ld:Vehicle:{namespace}-(A456|B9211|C7788)$")))
    );
    served.stop();
    Ok(())
}
