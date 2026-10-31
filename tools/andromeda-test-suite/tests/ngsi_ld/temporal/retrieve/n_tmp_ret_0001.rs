//! Deleted attributes read back as null instances under deleted at.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
    simplified_pairs::{language_pairs, object_pairs, property_pairs},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        AttributeKind,
        AttributeName,
        EntityId,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Relationship,
        Value,
        evolution::TemporalEntity,
    },
    query::temporal_query::TemporalProperty,
};

/// Pins: a Property, a Relationship and a `LanguageProperty` deleted through the Core API each read back under
/// timeproperty=deletedAt as the one instance holding the NGSI-LD Null and a deletedAt, normalized and in the simplified
/// representation
/// Clause: 4.5.7, 4.5.8, 4.5.9, 4.5.18, 4.11, 5.7.3.4, 5.6.5
#[gtest]
#[tokio::test]
async fn n_tmp_ret_0001_deleted_attributes_read_back_as_null_instances_under_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_p1_id = namespace.urn("Parking", "P1")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .relationship("isParkedIn", Relationship::builder(parking_p1_id).observed_at("2020-08-01T12:03:00Z".parse()?))
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "My Vehicle")])?).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    for name in ["fuelLevel", "isParkedIn", "name"] {
        broker
            .entities()
            .delete_attribute(&vehicle_id, &name.parse::<AttributeName>()?)
            .send()
            .await?
            .done()?;
    }
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&vehicle_id));
    for (name, kind) in [
        ("fuelLevel", AttributeKind::Property),
        ("isParkedIn", AttributeKind::Relationship),
        ("name", AttributeKind::LanguageProperty),
    ] {
        expect_that!(
            evolution,
            has_evolution(name, all!(len(eq(1)), nth(0, all!(attribute_of(kind), deleted_instance()))))
        );
    }
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .time_property(TemporalProperty::DeletedAt)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let values = retrieved_2.body();
    let fuel_level = property_pairs(values, "fuelLevel").or_fail()?;
    expect_that!(fuel_level, len(eq(1)));
    expect_that!(fuel_level.first().map(|pair| &pair.value), some(eq(&Value::from("urn:ngsi-ld:null"))));
    let is_parked_in = object_pairs(values, "isParkedIn").or_fail()?;
    expect_that!(is_parked_in, len(eq(1)));
    expect_that!(
        is_parked_in.first().map(|pair| &pair.value),
        some(eq(&OneOrMany::one("urn:ngsi-ld:null".parse::<EntityId>()?)))
    );
    let name = language_pairs(values, "name").or_fail()?;
    expect_that!(name, len(eq(1)));
    expect_that!(name.first().map(|pair| &pair.value), some(eq(&LanguageMap::ngsi_ld_null())));
    expect_that!(&retrieved_2.body().id, eq(&vehicle_id));
    Ok(())
}
