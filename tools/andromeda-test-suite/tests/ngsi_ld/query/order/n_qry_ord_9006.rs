//! A sub attribute and a compound member combine.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, JsonProperty, LanguageMap, LanguageProperty, NgsiLdMediaType, Property, RawJson, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: orderBy=sensor.rawdata`[particulate]` orders three Entities by the particulate member of the value of the
/// rawdata sub-Property of their sensor
/// Clause: 4.23.3, 4.9
#[gtest]
#[tokio::test]
async fn n_qry_ord_9006_a_sub_attribute_and_a_compound_member_combine() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let order_path_a_id = namespace.urn("OrderPath", "a")?;
    let order_path_type = namespace.entity_type("OrderPath")?;
    let order_path_b_id = namespace.urn("OrderPath", "b")?;
    let order_path_c_id = namespace.urn("OrderPath", "c")?;
    let entities = [
        Entity::builder()
            .id(order_path_a_id.clone())
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
            .id(order_path_b_id.clone())
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
            .id(order_path_c_id.clone())
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_path_type)
        .locality(Locality::Local)
        .ids(vec![order_path_a_id.clone(), order_path_b_id.clone(), order_path_c_id.clone()])
        .ordering("sensor.rawdata[particulate]".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&order_path_b_id)));
    expect_that!(page.items(), nth(1, has_id(&order_path_c_id)));
    expect_that!(page.items(), nth(2, has_id(&order_path_a_id)));
    Ok(())
}
