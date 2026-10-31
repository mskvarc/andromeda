//! Lists under each entry the attributes its own entities carry.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_includes},
    broker,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntityTypeDetails},
    model::{
        Entity,
        Property,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Details of Available Entity Types names under each type exactly the attributes its own entities
/// carry, a multi-typed entity's attributes under each of its types, and never the entity scope
/// Clause: 5.7.6.4, 5.2.25, 4.16, 6.25.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_tyd_0001_lists_under_each_entry_the_attributes_its_own_entities_carry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let store_3_id = namespace.urn("Store", 3)?;
    let store_4_id = namespace.urn("Store", 4)?;
    let building_5_id = namespace.urn("Building", 5)?;
    let building_type = namespace.entity_type("Building")?;
    let store_type = namespace.entity_type("Store")?;
    let marker = namespace.attribute("marker")?;
    let owner = namespace.attribute("owner")?;
    let position = namespace.attribute("position")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 1)
            .relationship(namespace.attribute("owner")?, building_2_id.clone())
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 2)
            .relationship(namespace.attribute("owner")?, building_id.clone())
            .geo_property(namespace.attribute("position")?, Shape::Point(Position::try_new(14.5, 46.05)?))
            .build()?,
        Entity::builder()
            .id(store_3_id)
            .entity_type(store_type.clone())
            .relationship(namespace.attribute("marker")?, building_id)
            .build()?,
        Entity::builder()
            .id(store_4_id)
            .entity_type(vec![building_type.clone(), store_type.clone()])
            .property(namespace.attribute("marker")?, 4)
            .property(
                namespace.attribute("marker")?,
                Property::builder(5).dataset_id(namespace.dataset_id("Dataset", "x")?),
            )
            .build()?,
        Entity::builder()
            .id(building_5_id)
            .entity_type(building_type.clone())
            .scope(format!("/Census/{namespace}").as_str())
            .property(namespace.attribute("marker")?, 6)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Sibling tests leave the reserved `ngsildproof` attribute in the tenant, which the typed list refuses, so the catalog is read as received.
    let client = broker.with_accept(Accept::any());
    let details = client.send_unchecked(client.unchecked(&RetrieveEntityTypeDetails::builder().build())?).await?;
    expect_that!(details.status(), eq(StatusCode::OK));
    let building = format!("$[?@.typeName == '{building_type}']");
    expect_that!(details.body(), json_equals(&format!("{building}.type"), "EntityType"));
    expect_that!(details.body(), json_count(&format!("{building}.attributeNames"), 3));
    expect_that!(details.body(), json_includes(&format!("{building}.attributeNames"), marker.to_string()));
    expect_that!(details.body(), json_includes(&format!("{building}.attributeNames"), owner.to_string()));
    expect_that!(details.body(), json_includes(&format!("{building}.attributeNames"), position.to_string()));
    let store = format!("$[?@.typeName == '{store_type}']");
    expect_that!(details.body(), json_equals(&format!("{store}.type"), "EntityType"));
    expect_that!(details.body(), json_count(&format!("{store}.attributeNames"), 1));
    expect_that!(details.body(), json_equals(&format!("{store}.attributeNames[0]"), marker.to_string()));
    Ok(())
}
