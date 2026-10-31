//! A temporal create holds the most current instance in current state.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instances},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        q,
        temporal_query::{self, TemporalProperty},
    },
};

/// Pins: Create Temporal Evolution of an Entity with v observed 50 on 2020-01-03, 10 on 2020-01-01 and 200 on
/// 2020-01-02 leaves Retrieve Entity answering v as one Property of value 50 without `instanceId`, `keyValues`
/// answering 50, q=v==50 selecting the Entity and q=v==200 nothing, and the Temporal Evolution holding all three
/// Clause: 4.5.5, 4.5.5.3, 5.6.11.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9011_a_temporal_create_holds_the_most_current_instance_in_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let temporal_create_id = namespace.urn("TemporalCreate", 1)?;
    let temporal_create_type = namespace.entity_type("TemporalCreate")?;
    let temporal_create_evolution = TemporalEntity::builder()
        .id(temporal_create_id.clone())
        .entity_type(temporal_create_type.clone())
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&temporal_create_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&temporal_create_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("v", 1));
    expect_that!(entity, has_attribute("v", any_attribute().value(50).without(Member::InstanceId)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&temporal_create_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("v", 50));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&temporal_create_type)
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&temporal_create_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&temporal_create_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&temporal_create_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["v".parse()?])
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(3)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(50)),
                contains(any_attribute().value(200))
            )
        )
    );
    Ok(())
}
