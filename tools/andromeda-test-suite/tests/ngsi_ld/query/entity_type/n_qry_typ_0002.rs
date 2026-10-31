//! One type selects every entity carrying it among others.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    type_matcher::{has_type, type_count},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a single type selects every entity carrying it, among other types too: type=Building answers the Building
/// and the `Building+TouristDestination` entity, not the Parking
/// Clause: 4.17, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_typ_0002_one_type_selects_every_entity_carrying_it_among_others() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let multi_types_id = namespace.urn("MultiTypes", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let multi_types_2_id = namespace.urn("MultiTypes", 2)?;
    let parking_type = namespace.entity_type("Parking")?;
    let multi_types_3_id = namespace.urn("MultiTypes", 3)?;
    let entities = [
        Entity::builder()
            .id(multi_types_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder().id(multi_types_2_id.clone()).entity_type(parking_type).build()?,
        Entity::builder()
            .id(multi_types_3_id.clone())
            .entity_type(vec![building_type.clone(), namespace.entity_type("TouristDestination")?])
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(all!(has_id(&multi_types_id), has_type(&building_type))));
    expect_that!(page.items(), contains(all!(has_id(&multi_types_3_id), type_count(2))));
    expect_that!(page.items(), not(contains(has_id(&multi_types_2_id))));
    Ok(())
}
