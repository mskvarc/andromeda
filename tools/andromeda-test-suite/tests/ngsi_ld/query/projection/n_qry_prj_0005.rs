//! A split query no source answers decides its filters before attrs or pick reduces the entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{EntitySplitting, projection::Projection, q, scope_query::ScopeQuery},
};

/// Pins: Query Entities by type with `splitEntities` true, attrs speed and q brand=="Mercedes", with no Context Source
/// registered, answers the Entity with speed 70 and brand Mercedes reduced to speed and not the one of brand Volvo;
/// with pick id,speed and `scopeQ` /Madrid it answers the Entity scoped /Madrid reduced to its id and speed with no
/// scope, and not the one scoped /Berlin
/// Clause: 5.7.2.4, 5.7.2.5, 4.21, 4.19
#[gtest]
#[tokio::test]
async fn n_qry_prj_0005_a_split_query_no_source_answers_decides_its_filters_before_the_list_reduces_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    for (id, brand, scope) in [(&vehicle_id, "Mercedes", "/Madrid"), (&vehicle_2_id, "Volvo", "/Berlin")] {
        let vehicle = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .scope(scope)
            .property("speed", 70)
            .property("brand", brand)
            .build()?;
        broker.entities().create(&vehicle).send().await?.done()?;
    }
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .q(q::attr("brand").eq("Mercedes"))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("brand")
        )]
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&vehicle_type)
        .pick("id,speed".parse::<Projection>()?)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(
        page_2.items(),
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("brand")
        )]
    );
    let item = page_2.items().first().or_fail()?;
    expect_that!(item.scope.as_ref(), none());
    Ok(())
}
