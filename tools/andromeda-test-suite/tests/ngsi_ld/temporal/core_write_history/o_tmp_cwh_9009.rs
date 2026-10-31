//! Merge entity records the deep merged value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, Entity, EntityFragment, NgsiLdMediaType, Property, Value, ValuePatch},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Merge Entity with address {street: Pariser Platz, country: urn:ngsi-ld:null} over address {street, city
/// Berlin, country} answers 204; the Entity holds address {street: Pariser Platz, city: Berlin}, the Temporal
/// Evolution holds two instances, one of them that merged value, and q selects address`[city]`=="Berlin" and
/// address`[street]`=="Pariser Platz"
/// Clause: 5.6.17, 5.5.12 (EXAMPLE 2), 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9009_merge_entity_records_the_deep_merged_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_address_id = namespace.urn("Building", "merge-address")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merge_address = Entity::builder()
        .id(building_merge_address_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([
                ("street", Value::from("Straße des 17. Juni")),
                ("city", Value::from("Berlin")),
                ("country", Value::from("Germany")),
            ]),
        )
        .build()?;
    broker.entities().create(&building_merge_address).send().await?.done()?;
    let address = ValuePatch::new().set("street", "Pariser Platz").delete("country");
    let fragment = EntityFragment::builder()
        .patch("address", Attribute::from(Property::merge_builder(address).build()?))
        .build()?;
    let merged = broker.with_responses().entities().merge(&building_merge_address_id, &fragment).send().await?;
    expect_that!(merged.status, eq(StatusCode::NO_CONTENT));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let merged_address = object([("street", Value::from("Pariser Platz")), ("city", Value::from("Berlin"))]);
    let retrieved = json.entities().retrieve(&building_merge_address_id).send().await?;
    expect_that!(retrieved.body(), has_attribute("address", any_attribute().value(merged_address.clone())));
    let retrieved_2 = json
        .temporal()
        .retrieve(&building_merge_address_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["address".parse()?])
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_evolution("address", all!(len(eq(2)), contains(any_attribute().value(merged_address))))
    );
    for (member, value) in [("city", "Berlin"), ("street", "Pariser Platz")] {
        let page = json
            .entities()
            .query()
            .entity_type(&building_type)
            .locality(Locality::Local)
            .q(q::attr("address").member(member).eq(value))
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(page.items(), contains(has_id(&building_merge_address_id)));
    }
    Ok(())
}
