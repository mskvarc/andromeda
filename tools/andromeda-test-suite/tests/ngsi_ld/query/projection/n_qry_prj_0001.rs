//! Pick keeps only listed members and omit removes only listed ones.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    spelling_matcher::alone,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Scope},
    query::projection::Projection,
};

/// Pins: Query Entities with pick naming no id, type or scope reduces each entity to the picked attribute alone,
/// dropping id, type and scope, and omit=type removes type alone, keeping id, scope and every attribute
/// Clause: 4.21, 5.7.2.5, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_prj_0001_pick_keeps_only_listed_members_and_omit_removes_only_listed_ones() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .property("speed", 42)
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![vehicle_id.clone()])
        .entity_type(&vehicle_type)
        .pick("speed".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(projected_member_count), some(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.scope.as_ref(), none());
    expect_that!(item.id, none());
    expect_that!(item.entity_type, none());
    expect_that!(
        page.items(),
        nth(0, all!(has_attribute("speed", any_attribute().value(42)), lacks_attribute("name")))
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![vehicle_id.clone()])
        .entity_type(&vehicle_type)
        .omit("type".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items().first().map(projected_member_count), some(eq(4)));
    let item_2 = page_2.items().first().or_fail()?;
    expect_that!(item_2.entity_type, none());
    expect_that!(item_2.scope.as_ref(), some(alone(&"/Madrid".parse::<Scope>()?)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            all!(
                has_id(&vehicle_id),
                has_attribute("speed", any_attribute().value(42)),
                has_attribute("name", any_attribute().value("Depot"))
            )
        )
    );
    Ok(())
}
