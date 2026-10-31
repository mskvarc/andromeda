//! The query after an update selects the new value.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Number, Value},
    query::{Locality, q},
};

/// Pins: Update Attributes of temperature 21.5 with 42.5 answers 204, and the Query Entities sent next with q
/// temperature==42.5 selects the Entity
/// Clause: 5.6.2, 5.7.2, 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9050_the_query_after_an_update_selects_the_new_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_atomicity_visible_id = namespace.urn("Building", "atomicity-visible")?;
    let building_type = namespace.entity_type("Building")?;
    let building_atomicity_visible = Entity::builder()
        .id(building_atomicity_visible_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_atomicity_visible).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", Value::Number("42.5".parse()?)).build()?;
    broker
        .entities()
        .update_attributes(&building_atomicity_visible_id, &fragment)
        .send()
        .await?
        .done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_atomicity_visible_id.clone()])
        .q(q::attr("temperature").eq("42.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_atomicity_visible_id)));
    Ok(())
}
