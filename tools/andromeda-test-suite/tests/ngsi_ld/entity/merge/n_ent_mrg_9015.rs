//! A merge naming no scope leaves it as it was.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Scope, Value},
};

/// Pins: Merge Entity with only a temperature leaves the scope `["/X/A","/Y/A"]` as it was
/// Clause: 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9015_a_merge_naming_no_scope_leaves_it_as_it_was() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_untouched_id = namespace.urn("Building", "untouched")?;
    let building_type = namespace.entity_type("Building")?;
    let building_untouched = Entity::builder()
        .id(building_untouched_id.clone())
        .entity_type(building_type)
        .scope(["/X/A", "/Y/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_untouched).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", 30).build()?;
    broker.entities().merge(&building_untouched_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_untouched_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/X/A".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/Y/A".parse::<Scope>()?)));
    Ok(())
}
