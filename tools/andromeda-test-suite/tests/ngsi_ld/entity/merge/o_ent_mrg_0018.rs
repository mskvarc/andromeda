//! A concise null deletes every relationship instance.

use andromeda_test_suite::{
    broker,
    entity_matcher::{has_instances, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        GeoProperty,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Property,
        Relationship,
        geometry::{Position, Shape},
    },
};

/// Pins: Merge Entity with a fragment whose `locatedAt` member is the concise NGSI-LD Null, on a Relationship with
/// a default instance and an instance with a `datasetId`, answers 204 and removes the whole `locatedAt` member,
/// both instances, leaving the other Attributes in place
/// Clause: 5.5.12, 5.6.17.4, 4.5.0, 4.5.5
/// Recorded: clause 5.6.17.4 says NGSI-LD Nulls "should" be supported by Merge Entity (`OperationNotSupported`
/// otherwise); Andromeda supports them, and the deletion then follows clause 5.6.17.4 and clause 5.5.12. Which
/// instances a bare "urn:ngsi-ld:null" member removes is ambiguous: clause 5.5.12 says that "For each member of the
/// Fragment, whose value is an NGSI-LD Null, contained by the target, the target member is removed", while clause
/// 5.6.17.4 targets the default instance when the merged Attribute carries no `datasetId`; Andromeda follows clause
/// 5.5.12 for the concise form and removes the member with every instance, and keeps the clause 5.6.17.4 reading
/// for a reified Attribute whose value is NGSI-LD Null
#[gtest]
#[tokio::test]
async fn o_ent_mrg_0018_a_concise_null_deletes_every_relationship_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let district_7th_id = namespace.urn("District", "7th")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property("name", Property::builder("La Tour Eiffel").dataset_id(namespace.dataset_id("Dataset", "02")?))
        .relationship("locatedAt", city_paris_id)
        .relationship(
            "locatedAt",
            Relationship::builder(district_7th_id).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2944, 48.8583)?)).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars"), ("fr", "Champ de Mars")])?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Avenue Gustave Eiffel"), ("fr", "Avenue Gustave Eiffel")])?)
                .dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder().delete("locatedAt").build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, lacks_attribute("locatedAt"));
    expect_that!(entity, has_instances("name", 2));
    expect_that!(entity, has_instances("location", 2));
    expect_that!(entity, has_instances("street", 2));
    Ok(())
}
