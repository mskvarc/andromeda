//! A q on unit code orders and matches by its text.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q ordering, a range and a pattern on `temperature.unitCode` and on
/// `temperature.accuracy.unitCode` selects by the unit code text, orders under the Unicode Collation Algorithm, and
/// never selects an entity without a `unitCode`
/// Clause: 4.5.2, 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9064_a_q_on_unit_code_orders_and_matches_by_its_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_cel_id = namespace.urn("Building", "cel")?;
    let building_type = namespace.entity_type("Building")?;
    let building_fah_id = namespace.urn("Building", "fah")?;
    let building_kel_id = namespace.urn("Building", "kel")?;
    let building_none_id = namespace.urn("Building", "none")?;
    let building_cel = Entity::builder()
        .id(building_cel_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .property("accuracy", Property::builder(Value::Number("0.5".parse()?)).unit_code(UnitCode::Cel)),
        )
        .build()?;
    broker.entities().create(&building_cel).send().await?.done()?;
    let building_fah = Entity::builder()
        .id(building_fah_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(68)
                .unit_code(UnitCode::Fah)
                .property("accuracy", Property::builder(1).unit_code(UnitCode::Fah)),
        )
        .build()?;
    broker.entities().create(&building_fah).send().await?.done()?;
    let building_kel = Entity::builder()
        .id(building_kel_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(293)
                .unit_code(UnitCode::Kel)
                .property("accuracy", Property::builder(Value::Number("0.5".parse()?)).unit_code(UnitCode::Kel)),
        )
        .build()?;
    broker.entities().create(&building_kel).send().await?.done()?;
    let building_none = Entity::builder()
        .id(building_none_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    broker.entities().create(&building_none).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").gt("CEL"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_fah_id)));
    expect_that!(page.items(), contains(has_id(&building_kel_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").le("CEL"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_cel_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").within("CEM", "FAI"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_fah_id)));
    // Under the Unicode Collation Algorithm "c" sorts before every code beginning with C or a later letter; code point order would put it after all of them.
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").gt("c"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(3)));
    expect_that!(page_4.items(), not(contains(has_id(&building_none_id))));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").matches("\"^.E\""))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), contains(has_id(&building_cel_id)));
    expect_that!(page_5.items(), contains(has_id(&building_kel_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").not_matches("\"^.E\""))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_fah_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("unitCode").ne("NOTACODE"))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(3)));
    expect_that!(page_7.items(), not(contains(has_id(&building_none_id))));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("accuracy").sub_attr("unitCode").ge("FAH"))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(2)));
    expect_that!(page_8.items(), contains(has_id(&building_fah_id)));
    expect_that!(page_8.items(), contains(has_id(&building_kel_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").sub_attr("accuracy").sub_attr("unitCode").matches("\"^C\""))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), nth(0, has_id(&building_cel_id)));
    Ok(())
}
