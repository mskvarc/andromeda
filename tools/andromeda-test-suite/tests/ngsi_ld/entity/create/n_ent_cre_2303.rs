//! A date time and a time value with a comma fraction are accepted.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, CoreDatatype, Datatype, Entity, NgsiLdMediaType, Property, TypedValue, Value},
};

/// Pins: Create Entity accepts a `DateTime` value and a Time value whose seconds fraction is separated by a comma,
/// and reads both back as the same instants
/// Clause: 4.6.3, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_2303_a_date_time_and_a_time_value_with_a_comma_fraction_are_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "serviceDate",
            Property::builder(Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-01T00:00:00,5Z")?)),
        )
        .property(
            "openingTime",
            Property::builder(Value::Typed(TypedValue::try_new(CoreDatatype::Time, "10:00:00,5Z")?)),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let typed = |name: &str| {
        entity
            .attributes
            .iter()
            .find(|(candidate, _)| candidate.as_str() == name)
            .and_then(|(_, instances)| match instances.first() {
                Attribute::Property(property) => match property.value() {
                    Value::Typed(typed) => Some(typed),
                    Value::Boolean(_) | Value::Number(_) | Value::Text(_) | Value::Array(_) | Value::Object(_) | Value::JsonNull => None,
                },
                Attribute::GeoProperty(_)
                | Attribute::LanguageProperty(_)
                | Attribute::VocabProperty(_)
                | Attribute::ListProperty(_)
                | Attribute::JsonProperty(_)
                | Attribute::Relationship(_)
                | Attribute::ListRelationship(_) => None,
            })
    };
    let service = typed("serviceDate").or_fail()?;
    expect_that!(service.datatype(), eq(&Datatype::Core(CoreDatatype::DateTime)));
    expect_that!(service.lexical(), matches_regex(r"^2024-01-01T00:00:00[.,]50*Z$"));
    let opening = typed("openingTime").or_fail()?;
    expect_that!(opening.datatype(), eq(&Datatype::Core(CoreDatatype::Time)));
    expect_that!(opening.lexical(), matches_regex(r"^10:00:00[.,]50*Z$"));
    Ok(())
}
