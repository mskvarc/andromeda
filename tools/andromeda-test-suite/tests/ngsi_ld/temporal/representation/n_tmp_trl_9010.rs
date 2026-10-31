//! A list relationship renders as object lists.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    simplified_pairs::{object_list_pairs, object_pairs},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{DateTime, ListRelationship, NgsiLdMediaType, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution with temporalValues of a `ListRelationship` with two instances answers type
/// `ListRelationship` and `objectLists` pairing each ordered `objectList` of URIs with its observedAt, and neither objects nor
/// object
/// Clause: 4.5.9, 4.5.22.2
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9010_a_list_relationship_renders_as_object_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_object_lists_id = namespace.urn("Building", "object-lists")?;
    let building_type = namespace.entity_type("Building")?;
    let person_alice_id = namespace.urn("Person", "alice")?;
    let person_bob_id = namespace.urn("Person", "bob")?;
    let first_day = "2022-08-09T18:25:02Z".parse::<DateTime>()?;
    let second_day = "2022-08-10T18:25:02Z".parse::<DateTime>()?;
    let building_object_lists_evolution = TemporalEntity::builder()
        .id(building_object_lists_id.clone())
        .entity_type(building_type)
        .list_relationship(
            "membersPresent",
            ListRelationship::builder(vec![person_alice_id.clone(), person_bob_id.clone()]).observed_at(first_day),
        )
        .list_relationship(
            "membersPresent",
            ListRelationship::builder(vec![person_alice_id.clone()]).observed_at(second_day),
        )
        .build()?;
    broker.temporal().create_or_update(&building_object_lists_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&building_object_lists_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let values = retrieved.body();
    let pairs = object_list_pairs(values, "membersPresent").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    let first = pairs.iter().find(|pair| pair.time.timestamp() == first_day.timestamp()).or_fail()?;
    expect_that!(&first.value, eq(&vec![person_alice_id.clone(), person_bob_id]));
    let second = pairs.iter().find(|pair| pair.time.timestamp() == second_day.timestamp()).or_fail()?;
    expect_that!(&second.value, eq(&vec![person_alice_id]));
    expect_that!(object_pairs(values, "membersPresent"), none());
    Ok(())
}
