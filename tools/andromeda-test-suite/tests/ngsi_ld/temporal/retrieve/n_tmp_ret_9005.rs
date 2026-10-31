//! A deleted list property reads back as a value list of one null.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, AttributeName, Entity, ListProperty, ListRelationship, NgsiLdMediaType, Value},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Retrieve Temporal Evolution by deletedAt of an Entity whose `ListProperty` was deleted answers the deletion as an
/// instance holding valueList [`"urn:ngsi-ld:null"`] beside deletedAt, and no value
/// Clause: 4.5.0, 4.5.7, 4.5.8, 4.5.21.2
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9005_a_deleted_list_property_reads_back_as_a_value_list_of_one_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_deleted_list_temporal_id = namespace.urn("Building", "deleted-list-temporal")?;
    let building_type = namespace.entity_type("Building")?;
    let building_deleted_list_temporal = Entity::builder()
        .id(building_deleted_list_temporal_id.clone())
        .entity_type(building_type)
        .list_property("tags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .list_relationship(
            "hasRooms",
            ListRelationship::builder(vec![namespace.urn("Room", 1)?, namespace.urn("Room", 2)?]),
        )
        .build()?;
    broker.entities().create(&building_deleted_list_temporal).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_deleted_list_temporal_id, &"tags".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_deleted_list_temporal_id)
        .temporal_q(temporal_query::after("1970-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::DeletedAt))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "tags",
            contains(all!(attribute_of(AttributeKind::ListProperty).list_length(1), deleted_instance()))
        )
    );
    Ok(())
}
