//! Every representation keeps a non ascii relationship object verbatim.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, EntityId, Lossless, NgsiLdMediaType},
};

/// Pins: Retrieve Entity answers the object of a Relationship naming an IRI with a non-ASCII character exactly as
/// written, normalized, concise and `keyValues`
/// Clause: 4.6.2, 4.5.1, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9018_every_representation_keeps_a_non_ascii_relationship_object_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_non_ascii_retrieval_id = namespace.urn("Building", "non-ascii-retrieval")?;
    let building_type = namespace.entity_type("Building")?;
    let ngsi_ld_ciudad_id = format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?;
    let building_non_ascii_retrieval = Entity::builder()
        .id(building_non_ascii_retrieval_id.clone())
        .entity_type(building_type)
        .relationship("locatedAt", format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?)
        .property("category", "office")
        .build()?;
    broker.entities().create(&building_non_ascii_retrieval).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_non_ascii_retrieval_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(ngsi_ld_ciudad_id.clone())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_non_ascii_retrieval_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("locatedAt", any_attribute().object(ngsi_ld_ciudad_id.clone())));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_non_ascii_retrieval_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_value("locatedAt", format!("urn:ngsi-ld:Ciudad:{namespace}-París")));
    Ok(())
}
