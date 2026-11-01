//! A declared number or Boolean reads back as a typed string in its canonical spelling.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: a Building whose `count` is "01" declared xsd:integer, `level` "+1.50" declared xsd:decimal, `scale`
/// "1.0E2" declared xsd:double, `open` "1" declared xsd:boolean, `peak` "INF" declared xsd:double and `floor` the
/// plain "01" beside valueType xsd:integer reads back "1", "1.5", "100", "true" and "INF" with their @type, and
/// `floor` as the plain "1"; `label`, "01" declared xsd:string, keeps its text
/// Clause: 4.5.2, 4.6.4, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9077_a_declared_number_or_boolean_reads_back_in_its_canonical_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("count", xsd_typed("integer", "01")?)
        .property("level", xsd_typed("decimal", "+1.50")?)
        .property("scale", xsd_typed("double", "1.0E2")?)
        .property("open", xsd_typed("boolean", "1")?)
        .property("peak", xsd_typed("double", "INF")?)
        .property("floor", Property::builder(Value::from("01")).value_type(xsd("integer")?))
        .property("label", xsd_typed("string", "01")?)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    for (name, local, canonical) in [
        ("count", "integer", "1"),
        ("level", "decimal", "1.5"),
        ("scale", "double", "100"),
        ("open", "boolean", "true"),
        ("peak", "double", "INF"),
        ("label", "string", "01"),
    ] {
        expect_that!(
            entity,
            has_attribute(name, any_attribute().value_member_as_written([], xsd_typed(local, canonical)?))
        );
    }
    expect_that!(entity, has_attribute("floor", any_attribute().value_member_as_written([], Value::from("1"))));
    Ok(())
}
