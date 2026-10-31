//! A dataset id keeps only the instances carrying it.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::{attribute_members, has_attribute, lacks_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Relationship},
};

/// Pins: Query Entities with a `datasetId` keeps, of every attribute, only the instance carrying it, and with attrs
/// only that attribute's instance
/// Clause: 5.7.2, 4.5.5, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_0007_a_dataset_id_keeps_only_the_instances_carrying_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let ville_paris_id = namespace.urn("Ville", "paris")?;
    let ciudad_paris_id = namespace.urn("Ciudad", "paris")?;
    let french_name_dataset = namespace.dataset_id("Dataset", "french-name")?;
    let german_name_dataset = namespace.dataset_id("Dataset", "german-name")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property(
            "name",
            Property::builder("Tour Eiffel").dataset_id(namespace.dataset_id("Dataset", "french-name")?),
        )
        .property(
            "name",
            Property::builder("Eiffelturm").dataset_id(namespace.dataset_id("Dataset", "german-name")?),
        )
        .relationship("locatedAt", city_paris_id)
        .relationship(
            "locatedAt",
            Relationship::builder(ville_paris_id.clone()).dataset_id(namespace.dataset_id("Dataset", "french-name")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(ciudad_paris_id).dataset_id(namespace.dataset_id("Dataset", "spanish-name")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .dataset_ids(vec![french_name_dataset.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(member_count), some(eq(4)));
    expect_that!(page.items().first().and_then(|item| attribute_members(item, "name")), some(eq(3)));
    expect_that!(page.items().first().and_then(|item| attribute_members(item, "locatedAt")), some(eq(3)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("name", property().value("Tour Eiffel").with_dataset_id(french_name_dataset.clone())),
                has_attribute(
                    "locatedAt",
                    relationship().object(ville_paris_id.clone()).with_dataset_id(french_name_dataset.clone())
                ),
                has_type(&building_type)
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .attrs(vec!["name".parse()?])
        .dataset_ids(vec![german_name_dataset.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items().first().map(member_count), some(eq(3)));
    expect_that!(page_2.items().first().and_then(|item| attribute_members(item, "name")), some(eq(3)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("name", property().value("Eiffelturm").with_dataset_id(german_name_dataset.clone())),
                lacks_attribute("locatedAt"),
                has_type(&building_type)
            )
        )
    );
    Ok(())
}
