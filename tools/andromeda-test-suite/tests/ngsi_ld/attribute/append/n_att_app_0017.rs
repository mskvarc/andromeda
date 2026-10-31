//! Appends of distinct attributes sent at once to one entity are all held.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// The Append Attributes requests sent at once, each appending an attribute of its own.
const PARALLEL_APPENDS: u32 = 8;

/// Pins: eight Append Attributes of one Entity sent at once, each appending a distinct attribute, each answer 204,
/// and the Entity then holds its original attribute and all eight appended ones with their values
/// Clause: 5.6.3, 6.6.3.1
#[gtest]
#[tokio::test]
async fn n_att_app_0017_appends_of_distinct_attributes_sent_at_once_to_one_entity_are_all_held() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(building_id.clone()) // clone: the Building is appended to and retrieved by its id after it is built
        .entity_type(namespace.entity_type("Building")?)
        .property("temperature", 0)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let names: Vec<String> = (0..PARALLEL_APPENDS).map(|index| format!("attr{index}")).collect();
    let fragments = (0_u32..)
        .zip(&names)
        .map(|(value, name)| Ok(EntityFragment::builder().property(name.as_str(), value).build()?))
        .collect::<Result<Vec<_>>>()?;
    let responding = broker.with_responses();
    let entities = responding.entities();
    let answers = join_all(fragments.iter().map(|fragment| entities.append_attributes(&building_id, fragment).send())).await;
    expect_that!(
        answers.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::NO_CONTENT)))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.attributes.len(), eq(names.len() + 1));
    expect_that!(entity, has_attribute("temperature", any_attribute().value(0)));
    for (value, name) in (0_u32..).zip(&names) {
        expect_that!(entity, has_attribute(name, any_attribute().value(value)));
    }
    Ok(())
}
