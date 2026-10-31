//! Without a dataset id every instance is answered.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::{has_instance, has_instances, lacks_attribute, member_count},
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

/// Pins: Query Entities with attrs and no `datasetId` answers every instance of the selected multi-instance
/// attribute
/// Clause: 5.7.2, 4.5.5
#[gtest]
#[tokio::test]
async fn n_qry_ent_0009_without_a_dataset_id_every_instance_is_answered() -> Result<()> {
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
            Relationship::builder(ville_paris_id).dataset_id(namespace.dataset_id("Dataset", "french-name")?),
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
        .attrs(vec!["name".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(member_count), some(eq(3)));
    expect_that!(page.items(), nth(0, has_instances("name", 3)));
    let item = page.items().first().or_fail()?;
    expect_that!(item, has_instance("name", property().value("Eiffel Tower")));
    expect_that!(item, has_instance("name", any_attribute().value("Eiffel Tower").without(Member::DatasetId)));
    expect_that!(
        item,
        has_instance("name", any_attribute().value("Tour Eiffel").with_dataset_id(french_name_dataset))
    );
    expect_that!(
        item,
        has_instance("name", any_attribute().value("Eiffelturm").with_dataset_id(german_name_dataset))
    );
    expect_that!(
        page.items(),
        nth(0, all!(has_id(&building_id), lacks_attribute("locatedAt"), has_type(&building_type)))
    );
    Ok(())
}
