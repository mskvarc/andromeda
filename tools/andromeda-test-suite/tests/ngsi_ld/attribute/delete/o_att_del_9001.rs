//! A core deletion after a temporal create records one deletion.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};

/// Pins: Delete Attribute v of an Entity created through Create Temporal Evolution with three instances records
/// exactly one instance with deletedAt and the value urn:ngsi-ld:null
/// Clause: 4.5.7, 4.5.8, 5.6.5
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API and
/// leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in step with
/// the Temporal Evolution, so the Entity a Temporal API write addresses has current state
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_att_del_9001_a_core_deletion_after_a_temporal_create_records_one_deletion() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let temporal_then_core_id = namespace.urn("TemporalThenCore", 1)?;
    let temporal_then_core_type = namespace.entity_type("TemporalThenCore")?;
    let temporal_then_core = TemporalEntity::builder()
        .id(temporal_then_core_id.clone())
        .entity_type(temporal_then_core_type)
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&temporal_then_core).send().await?.created()?;
    broker
        .entities()
        .delete_attribute(&temporal_then_core_id, &"v".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&temporal_then_core_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("v", all!(len(eq(1)), nth(0, any_attribute().with_member(Member::DeletedAt).null_value())))
    );
    Ok(())
}
