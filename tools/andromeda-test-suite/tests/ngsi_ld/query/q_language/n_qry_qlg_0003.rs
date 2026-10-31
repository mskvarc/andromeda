//! An unquoted uri matches a relationship object.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Property, Uri},
    query::q,
};

/// Pins: q alone (no type) compares a Relationship object with an unquoted URI: `locatedIn==<pisa>` selects the
/// Building located there, a city no Building names selects nothing, and an OR of both selects the one Building
/// Clause: 4.9, 5.7.2, 6.3.13
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0003_an_unquoted_uri_matches_a_relationship_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let city_pisa_id = namespace.urn("City", "pisa")?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(6).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
            )
            .property("almostFull", true)
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .property("subCategory", "sightseeing")
            .property(
                "airQualityLevel",
                Property::builder(7).unit_code(UnitCode::C62).observed_at("2023-10-10T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .relationship(namespace.attribute("locatedIn")?, city_pisa_id)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // q is the only selector; the Relationship name carries the namespace, so only this file's entities can hold it
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .q(q::attr(&namespace.attribute("locatedIn")?).eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:City:{namespace}-pisa").parse::<Uri>()?)?))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .q(q::attr(&namespace.attribute("locatedIn")?).eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:City:{namespace}-paris").parse::<Uri>()?)?))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(0)));
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .q(q::attr(&namespace.attribute("locatedIn")?)
            .eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:City:{namespace}-pisa").parse::<Uri>()?)?)
            .or(q::attr(&namespace.attribute("locatedIn")?).eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:City:{namespace}-paris").parse::<Uri>()?)?)))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(1)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_3_id)));
    Ok(())
}
