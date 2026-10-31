//! Ordering reads an attribute held only by a dataset instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=rank` ranks the entity whose only rank instance carries a `datasetId` by that value, ahead of a
/// larger default rank and of an entity without rank
/// Clause: 4.23.2, 4.23.3, 4.5.5
/// Recorded: the entity holding rank 1 under a `datasetId` holds the attribute, so 4.23.2's "Attribute does not
/// exist" (last) does not apply to it; with one instance there is no choice between values to make
#[gtest]
#[tokio::test]
async fn n_qry_ord_4008_ordering_reads_an_attribute_held_only_by_a_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_none_id = namespace.urn("Building", "none")?;
    let building_type = namespace.entity_type("Building")?;
    let building_default_id = namespace.urn("Building", "default")?;
    let building_dataset_id = namespace.urn("Building", "dataset")?;
    let entities = [
        Entity::builder().id(building_none_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_default_id.clone())
            .entity_type(building_type.clone())
            .property("rank", 2)
            .build()?,
        Entity::builder()
            .id(building_dataset_id.clone())
            .entity_type(building_type.clone())
            .property("rank", Property::builder(1).dataset_id(namespace.dataset_id("Dataset", "survey")?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("rank".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_dataset_id)));
    expect_that!(page.items(), nth(1, has_id(&building_default_id)));
    expect_that!(page.items(), nth(2, has_id(&building_none_id)));
    Ok(())
}
