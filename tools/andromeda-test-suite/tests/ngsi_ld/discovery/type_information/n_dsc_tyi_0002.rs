//! Is not found once every entity is deleted.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, type_information::attribute_names};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
};

/// Pins: deleting an entity lowers its type's `entityCount`, and once every entity of the type is deleted the type
/// information is `ResourceNotFound` and neither the type nor its attributes are listed any more
/// Clause: 5.7.7.4, 5.2.26, 6.26.3.1, 5.7.5.4, 5.7.8.4
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0002_is_not_found_once_every_entity_is_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let owner = namespace.attribute("owner")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 2)
            .relationship(namespace.attribute("owner")?, building_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker.entities().delete(&building_id).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(1));
    expect_that!(information.attribute_details, len(eq(2)));
    expect_that!(attribute_names(&information), unordered_elements_are![eq(&height), eq(&owner)]);
    broker.entities().delete(&building_2_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entity_types()
        .retrieve(&building_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let client = broker.with_accept(Accept::any());
    let types = client.with_heads().entity_types().list().send().await?;
    expect_that!(types.status, eq(StatusCode::OK));
    expect_that!(types.outcome.type_list, not(contains(eq(&building_type))));
    let attributes = client.attributes().list().send().await?;
    expect_that!(attributes.attribute_list, not(contains(eq(&height))));
    expect_that!(attributes.attribute_list, not(contains(eq(&owner))));
    let details = client.entity_types().list_details().send().await?;
    expect_that!(details.iter().find(|item| item.type_name == building_type), none());
    Ok(())
}
