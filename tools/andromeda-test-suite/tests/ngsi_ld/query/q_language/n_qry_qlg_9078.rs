//! A q equality includes a declared value held among several values by the data type its declaration gives it.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeDraft, DateTime, Entity, EntityId, EntityTypeName, Input, JsonProperty, ListProperty, NgsiLdMediaType, Property, RawJson, Value},
    query::q::{self, builders::QueryBuilder},
};

/// The ids, sorted, of the entities of `building_type` the query `q` selects.
async fn selected(broker: &BrokerClient, building_type: &EntityTypeName, q: QueryBuilder) -> Result<Vec<String>> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(building_type)
        .q(q)
        .send()
        .await?;
    let mut ids: Vec<String> = page.items().iter().map(|entity| entity.id.to_string()).collect();
    ids.sort();
    Ok(ids)
}

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// The Building `id` whose Property `name` is `property`.
fn building(id: &EntityId, building_type: &EntityTypeName, name: &str, property: impl AttributeDraft<Property<Input>>) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(id.clone())
        .entity_type(building_type.clone())
        .property(name, property)
        .build()?)
}

/// The array of `values`.
fn array<const N: usize>(values: [Value; N]) -> Value {
    Value::Array(values.into())
}

/// Pins: over Buildings whose `reading` holds `["4","7"]` declared xsd:integer by each @type, `["4","7"]` declared
/// xsd:integer by the valueType, `[4,7]`, the plain `["4","7"]`, `["true","false"]` declared xsd:boolean by each @type,
/// `["1","0"]` declared xsd:boolean by the valueType, `["INF","NaN"]` declared xsd:double, and a `ListProperty` whose
/// `valueList` is `["4"]` declared xsd:integer, and two `JsonProperty` attributes, `[{"@value":"4"},{"a":1}]` beside the
/// valueType xsd:integer and `[{"@value":4}]`, `==4` selects both declared integers, the numbers and the list, `!=4`
/// the rest, the `JsonProperty` attributes among them, `=="4"` only the plain strings, `!="4"` all but them, `==true` both declared Booleans, `=="true"` none,
/// `!=true` all but the declared Booleans, and `>3` none; over Buildings whose `level` holds the sub-attribute `raw`,
/// `level.raw==4` selects `["4","7"]` declared xsd:integer by the valueType and by each @type, `level.raw=="4"` none,
/// and `level.raw==2024-01-01T00:00:00Z` selects only the times declared `DateTime` by the valueType, which
/// `level.raw=="2024-01-01T00:00:00Z"` does not; a `raw` `JsonProperty` holding `[{"@value":"abc"}]` beside the
/// valueType xsd:integer is written, and no equality selects it
/// Clause: 4.9, 4.5.2, 4.5.24, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9078_a_q_equality_includes_a_declared_value_held_among_several_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let typed_integers_id = namespace.urn("Building", "typed-integers")?;
    let value_type_integers_id = namespace.urn("Building", "value-type-integers")?;
    let numbers_id = namespace.urn("Building", "numbers")?;
    let strings_id = namespace.urn("Building", "strings")?;
    let typed_booleans_id = namespace.urn("Building", "typed-booleans")?;
    let value_type_booleans_id = namespace.urn("Building", "value-type-booleans")?;
    let non_finite_id = namespace.urn("Building", "non-finite")?;
    let list_id = namespace.urn("Building", "list")?;
    let raw_value_type_id = namespace.urn("Building", "raw-value-type")?;
    let raw_typed_id = namespace.urn("Building", "raw-typed")?;
    let raw_times_id = namespace.urn("Building", "raw-times")?;
    let json_declared_id = namespace.urn("Building", "json-declared")?;
    let json_node_id = namespace.urn("Building", "json-node")?;
    let raw_json_id = namespace.urn("Building", "raw-json")?;
    let date_time = "https://uri.etsi.org/ngsi-ld/DateTime".parse()?;
    let entities = [
        building(
            &typed_integers_id,
            &building_type,
            "reading",
            Property::builder(array([xsd_typed("integer", "4")?, xsd_typed("integer", "7")?])),
        )?,
        building(
            &value_type_integers_id,
            &building_type,
            "reading",
            Property::builder(array([Value::from("4"), Value::from("7")])).value_type(xsd("integer")?),
        )?,
        building(
            &numbers_id,
            &building_type,
            "reading",
            Property::builder(array([Value::from(4), Value::from(7)])),
        )?,
        building(
            &strings_id,
            &building_type,
            "reading",
            Property::builder(array([Value::from("4"), Value::from("7")])),
        )?,
        building(
            &typed_booleans_id,
            &building_type,
            "reading",
            Property::builder(array([xsd_typed("boolean", "true")?, xsd_typed("boolean", "false")?])),
        )?,
        building(
            &value_type_booleans_id,
            &building_type,
            "reading",
            Property::builder(array([Value::from("1"), Value::from("0")])).value_type(xsd("boolean")?),
        )?,
        building(
            &non_finite_id,
            &building_type,
            "reading",
            Property::builder(array([xsd_typed("double", "INF")?, xsd_typed("double", "NaN")?])),
        )?,
        Entity::builder()
            .id(list_id.clone())
            .entity_type(building_type.clone())
            .list_property("reading", ListProperty::builder(vec![xsd_typed("integer", "4")?]))
            .build()?,
        Entity::builder()
            .id(json_declared_id.clone())
            .entity_type(building_type.clone())
            .json_property(
                "reading",
                JsonProperty::builder(r#"[{"@value":"4"},{"a":1}]"#.parse::<RawJson>()?).value_type(xsd("integer")?),
            )
            .build()?,
        Entity::builder()
            .id(json_node_id.clone())
            .entity_type(building_type.clone())
            .json_property("reading", JsonProperty::builder(r#"[{"@value":4}]"#.parse::<RawJson>()?))
            .build()?,
        building(
            &raw_json_id,
            &building_type,
            "level",
            Property::builder(1).json_property(
                "raw",
                JsonProperty::builder(r#"[{"@value":"abc"}]"#.parse::<RawJson>()?).value_type(xsd("integer")?),
            ),
        )?,
        building(
            &raw_value_type_id,
            &building_type,
            "level",
            Property::builder(1).property(
                "raw",
                Property::builder(array([Value::from("4"), Value::from("7")])).value_type(xsd("integer")?),
            ),
        )?,
        building(
            &raw_typed_id,
            &building_type,
            "level",
            Property::builder(1).property("raw", Property::builder(array([xsd_typed("integer", "4")?, xsd_typed("integer", "7")?]))),
        )?,
        building(
            &raw_times_id,
            &building_type,
            "level",
            Property::builder(1).property(
                "raw",
                Property::builder(array([Value::from("2024-01-01T00:00:00Z"), Value::from("2024-06-01T00:00:00Z")])).value_type(date_time),
            ),
        )?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let reading = || q::attr("reading");
    let readings = [
        &typed_integers_id,
        &value_type_integers_id,
        &numbers_id,
        &strings_id,
        &typed_booleans_id,
        &value_type_booleans_id,
        &non_finite_id,
        &list_id,
        &json_declared_id,
        &json_node_id,
    ];
    let readings_but = |excluded: &[&EntityId]| sorted(&readings.iter().copied().filter(|id| !excluded.contains(id)).collect::<Vec<_>>());
    let fours = [&typed_integers_id, &value_type_integers_id, &numbers_id, &list_id];
    let booleans = [&typed_booleans_id, &value_type_booleans_id];
    expect_that!(selected(&broker, &building_type, reading().eq(4)).await?, eq(&sorted(&fours)));
    // Clause 4.9: an array that does not include the operand, or includes it only as another data type, is unequal.
    expect_that!(selected(&broker, &building_type, reading().ne(4)).await?, eq(&readings_but(&fours)));
    expect_that!(selected(&broker, &building_type, reading().eq("4")).await?, eq(&sorted(&[&strings_id])));
    expect_that!(selected(&broker, &building_type, reading().ne("4")).await?, eq(&readings_but(&[&strings_id])));
    expect_that!(selected(&broker, &building_type, reading().eq(true)).await?, eq(&sorted(&booleans)));
    expect_that!(selected(&broker, &building_type, reading().eq("true")).await?, is_empty());
    expect_that!(selected(&broker, &building_type, reading().ne(true)).await?, eq(&readings_but(&booleans)));
    // Clause 4.9 gives an array no ordering.
    expect_that!(selected(&broker, &building_type, reading().gt(3)).await?, is_empty());
    let raw = || q::attr("level").sub_attr("raw");
    expect_that!(
        selected(&broker, &building_type, raw().eq(4)).await?,
        eq(&sorted(&[&raw_value_type_id, &raw_typed_id]))
    );
    expect_that!(selected(&broker, &building_type, raw().eq("4")).await?, is_empty());
    expect_that!(selected(&broker, &building_type, raw().eq("abc")).await?, is_empty());
    expect_that!(
        selected(&broker, &building_type, raw().eq("2024-01-01T00:00:00Z".parse::<DateTime>()?)).await?,
        eq(&sorted(&[&raw_times_id]))
    );
    expect_that!(selected(&broker, &building_type, raw().eq("2024-01-01T00:00:00Z")).await?, is_empty());
    Ok(())
}
