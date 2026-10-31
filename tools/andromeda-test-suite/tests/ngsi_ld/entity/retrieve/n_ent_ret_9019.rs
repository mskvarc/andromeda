//! A multi instance relationship keeps a non ascii object verbatim.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::has_value_at,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, EntityId, NgsiLdMediaType, Relationship},
};

/// Pins: Retrieve Entity of a Relationship with two `datasetId` instances answers the object of the instance naming
/// an IRI with a non-ASCII character exactly as written, normalized and in the `keyValues` dataset map
/// Clause: 4.6.2, 4.5.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ent_ret_9019_a_multi_instance_relationship_keeps_a_non_ascii_object_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_non_ascii_multi_retrieval_id = namespace.urn("Building", "non-ascii-multi-retrieval")?;
    let building_type = namespace.entity_type("Building")?;
    let city_london_id = namespace.urn("City", "London")?;
    let spanish_name_dataset = namespace.dataset_id("Dataset", "spanish-name")?;
    let ngsi_ld_ciudad_id = format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?;
    let building_non_ascii_multi_retrieval = Entity::builder()
        .id(building_non_ascii_multi_retrieval_id.clone())
        .entity_type(building_type)
        .relationship(
            "locatedAt",
            Relationship::builder(city_london_id).dataset_id(namespace.dataset_id("Dataset", "ascii")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?)
                .dataset_id(namespace.dataset_id("Dataset", "spanish-name")?),
        )
        .property("category", "office")
        .build()?;
    broker.entities().create(&building_non_ascii_multi_retrieval).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_non_ascii_multi_retrieval_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("locatedAt", 2));
    expect_that!(
        entity,
        has_attribute("locatedAt", any_attribute().dataset_id(spanish_name_dataset).object(ngsi_ld_ciudad_id))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_non_ascii_multi_retrieval_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_value_at(
            "locatedAt",
            ["dataset", format!("urn:ngsi-ld:Dataset:{namespace}-spanish-name").as_str()],
            format!("urn:ngsi-ld:Ciudad:{namespace}-París")
        )
    );
    Ok(())
}
