//! A temporal evolution renders every sub attribute it recorded.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, geo_property, property, relationship},
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        Entity,
        EntityFragment,
        EntityId,
        Input,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabTerm,
        geometry::{BoundingBox, Geometry, Position, Shape},
    },
};
use serde_json::json;

/// Pins: Create Entity whose speed carries a Property accuracy with unitCode, a Relationship source with objectType, a
/// `GeoProperty` where with bbox, a `LanguageProperty` label, a `JsonProperty` raw and a `ListProperty` steps, then Update
/// Attributes of speed with the same sub-attributes, reads back two speed instances each holding every sub-attribute and
/// member as written
/// Clause: 5.7.3, 4.5.2, 4.5.6
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9042_a_temporal_evolution_renders_every_sub_attribute_it_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "every-sub-attribute")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = VocabTerm::from(&namespace.entity_type("Sensor")?);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", speed(20, "2024-01-01T00:00:00Z", sensor_id.clone(), sensor_type.clone())?)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", speed(21, "2024-01-02T00:00:00Z", sensor_id.clone(), sensor_type.clone())?)
        .build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let ninety = Value::Number("0.9".parse()?);
    let accuracy = || property().value(ninety.clone()).unit_code(UnitCode::Mtr).without(Member::CreatedAt);
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                each(
                    any_attribute()
                        .sub_attribute("accuracy", accuracy())
                        .sub_attribute("source", relationship().object(sensor_id.clone()).object_type(sensor_type.clone()))
                        .sub_attribute("where", geo_property().geometry(corner()?))
                        .sub_attribute(
                            "label",
                            attribute_of(AttributeKind::LanguageProperty).language("en", "fast").language("de", "schnell")
                        )
                        .sub_attribute("raw", attribute_of(AttributeKind::JsonProperty).json(json!({"frame": [1, 2]})))
                        .sub_attribute("steps", attribute_of(AttributeKind::ListProperty).list_length(3))
                ),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(21))
            )
        )
    );
    Ok(())
}

/// The point the `where` sub-attribute holds, with its bounding box.
fn corner() -> Result<Geometry> {
    let point = Position::try_new(13.5, 52.25)?;
    Ok(Geometry::from(Shape::Point(point.clone())).with_bbox(BoundingBox::try_new(point.clone(), point)?)?)
}

/// The `speed` instance of `value` observed at `day`, carrying one sub-attribute of each kind.
fn speed(value: i32, day: &str, sensor_id: EntityId, sensor_type: VocabTerm) -> Result<Property<Input>> {
    Ok(Property::builder(value)
        .observed_at(day.parse()?)
        .property("accuracy", Property::builder(Value::Number("0.9".parse()?)).unit_code(UnitCode::Mtr))
        .relationship("source", Relationship::builder(sensor_id).object_type(sensor_type))
        .geo_property("where", corner()?)
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "fast"), ("de", "schnell")])?),
        )
        .json_property("raw", JsonProperty::builder(r#"{"frame": [1, 2]}"#.parse::<RawJson>()?))
        .list_property("steps", vec![Value::from(1), Value::from(2), Value::from(3)])
        .build()?)
}
