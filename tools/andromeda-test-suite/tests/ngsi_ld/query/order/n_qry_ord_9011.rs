//! A linked entity traversal is not an ordering term.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, value::object, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, JsonProperty, LanguageMap, LanguageProperty, NgsiLdMediaType, Property, RawJson, Value},
};

/// Pins: `orderBy=rel{name}` and orderBy=rel{Building,Store:name} answer 400 `BadRequestData`
/// Clause: 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9011_a_linked_entity_traversal_is_not_an_ordering_term() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let order_path_a_id = namespace.urn("OrderPath", "a")?;
    let order_path_type = namespace.entity_type("OrderPath")?;
    let order_path_b_id = namespace.urn("OrderPath", "b")?;
    let order_path_c_id = namespace.urn("OrderPath", "c")?;
    let entities = [
        Entity::builder()
            .id(order_path_a_id)
            .entity_type(order_path_type.clone())
            .property(
                "address",
                object([("city", Value::from("Amsterdam")), ("street", Value::from("Ulrich Strasse"))]),
            )
            .property("name", Property::builder("name-a").observed_at("2024-03-01T00:00:00Z".parse()?))
            .json_property("blob", JsonProperty::builder(r#"{"k":"alpha"}"#.parse::<RawJson>()?))
            .language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "zebra"), ("de", "zzz")])?),
            )
            .property(
                "sensor",
                Property::builder(40).property("rawdata", object([("particulate", Value::from("gamma"))])),
            )
            .build()?,
        Entity::builder()
            .id(order_path_b_id)
            .entity_type(order_path_type.clone())
            .property("address", object([("city", Value::from("Berlin")), ("street", Value::from("Ulrich Strasse"))]))
            .property("name", Property::builder("name-b").observed_at("2024-01-01T00:00:00Z".parse()?))
            .json_property("blob", JsonProperty::builder(r#"{"k":"charlie"}"#.parse::<RawJson>()?))
            .language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "apple"), ("de", "zzz")])?),
            )
            .property(
                "sensor",
                Property::builder(40).property("rawdata", object([("particulate", Value::from("alpha"))])),
            )
            .build()?,
        Entity::builder()
            .id(order_path_c_id)
            .entity_type(order_path_type.clone())
            .property(
                "address",
                object([("city", Value::from("Copenhagen")), ("street", Value::from("Ulrich Strasse"))]),
            )
            .property("name", Property::builder("name-c").observed_at("2024-02-01T00:00:00Z".parse()?))
            .json_property("blob", JsonProperty::builder(r#"{"k":"bravo"}"#.parse::<RawJson>()?))
            .language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "mango"), ("de", "zzz")])?),
            )
            .property(
                "sensor",
                Property::builder(40).property("rawdata", object([("particulate", Value::from("beta"))])),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=OrderPath_{namespace}&local=true&id=urn%3Angsi-ld%3AOrderPath%3A{namespace}-a%2Curn%3Angsi-ld%3AOrderPath%3A{namespace}-b%2Curn%3Angsi-ld%3AOrderPath%3A{namespace}-c&orderBy=rel%7Bname%7D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=OrderPath_{namespace}&local=true&id=urn%3Angsi-ld%3AOrderPath%3A{namespace}-a%2Curn%3Angsi-ld%3AOrderPath%3A{namespace}-b%2Curn%3Angsi-ld%3AOrderPath%3A{namespace}-c&orderBy=rel%7BBuilding%2CStore%3Aname%7D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
