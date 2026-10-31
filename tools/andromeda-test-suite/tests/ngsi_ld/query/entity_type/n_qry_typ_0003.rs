//! A parenthesised conjunction selects only entities of every type.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::{lists_type, type_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::type_selection,
};

/// Pins: a parenthesised conjunction (Building;TouristDestination) selects only the entity carrying both types
/// Clause: 4.17, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_typ_0003_a_parenthesised_conjunction_selects_only_entities_of_every_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let multi_types_id = namespace.urn("MultiTypes", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let multi_types_2_id = namespace.urn("MultiTypes", 2)?;
    let parking_type = namespace.entity_type("Parking")?;
    let multi_types_3_id = namespace.urn("MultiTypes", 3)?;
    let tourist_destination_type = namespace.entity_type("TouristDestination")?;
    let entities = [
        Entity::builder()
            .id(multi_types_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder().id(multi_types_2_id).entity_type(parking_type).build()?,
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
        .entity_type(type_selection::all_of([&building_type, &namespace.entity_type("TouristDestination")?]).build()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&multi_types_3_id),
                type_count(2),
                lists_type(&building_type),
                lists_type(&tourist_destination_type)
            )
        )
    );
    Ok(())
}
