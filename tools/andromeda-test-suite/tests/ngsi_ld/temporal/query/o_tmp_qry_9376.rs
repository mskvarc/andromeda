//! System time reads order a modified instance and a deletion by their own instants.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    instance_time::{instance_time_at, instance_time_where},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    system_time::SystemMember,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: of a gauge written 1, 2 and 3 by Core writes, the instance holding 1 modified to 10 and the attribute then
/// deleted, the `modifiedAt` evolution lists 2, 3 and 10 in that order and `lastN` 1 on `modifiedAt` and on
/// `createdAt` each answers one instance, the `createdAt` evolution keeps 10 first with the `createdAt` it was written
/// at
/// Clause: 4.8, 5.6.14.4, 5.7.3.4, 5.2.21, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9376_system_time_reads_order_a_modified_instance_and_a_deletion_by_their_own_instants() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gauge_id = namespace.urn("Gauge", 1)?;
    let gauge_type = namespace.entity_type("Gauge")?;
    let level = "level".parse::<AttributeName>()?;
    let gauge = Entity::builder()
        .id(gauge_id.clone())
        .entity_type(gauge_type)
        .property("level", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&gauge).send().await?.done()?;
    for (value, observed_at) in [(2, "2024-01-01T01:00:00Z"), (3, "2024-01-01T02:00:00Z")] {
        let fragment = EntityFragment::builder()
            .property("level", Property::builder(value).observed_at(observed_at.parse()?))
            .build()?;
        broker.entities().update_attributes(&gauge_id, &fragment).send().await?.done()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.temporal().retrieve(&gauge_id).system_attributes(SystemAttributes::Include).send().await?;
    let before = retrieved.body();
    let instance_id = instance_id_where(before, &"level".parse()?, any_attribute().value(1))?;
    let created = instance_time_where(before, &"level".parse()?, any_attribute().value(1), SystemMember::CreatedAt)?;
    let instance = Attribute::from(Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?).build()?);
    let modified = broker
        .with_responses()
        .temporal()
        .modify_instance(&gauge_id, &level, &instance_id, &instance)
        .send()
        .await;
    expect_that!(&modified, answered(StatusCode::NO_CONTENT));
    broker.entities().delete_attribute(&gauge_id, &level).send().await?.done()?;
    let by_modified = json
        .temporal()
        .retrieve(&gauge_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let modified_order = by_modified.body();
    expect_that!(
        modified_order,
        has_evolution(
            "level",
            all!(
                nth(0, any_attribute().value(2)),
                nth(1, any_attribute().value(3)),
                nth(2, any_attribute().value(10).instance_id(&instance_id))
            )
        )
    );
    expect_that!(&instance_time_at(modified_order, &"level".parse()?, 2, SystemMember::CreatedAt)?, eq(&created));
    let by_created = json
        .temporal()
        .retrieve(&gauge_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::CreatedAt))
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let created_order = by_created.body();
    expect_that!(
        created_order,
        has_evolution(
            "level",
            all!(
                nth(0, any_attribute().value(10)),
                nth(1, any_attribute().value(2)),
                nth(2, any_attribute().value(3))
            )
        )
    );
    expect_that!(&instance_time_at(created_order, &"level".parse()?, 0, SystemMember::CreatedAt)?, eq(&created));
    for time_property in [TemporalProperty::ModifiedAt, TemporalProperty::CreatedAt] {
        let last = json
            .temporal()
            .retrieve(&gauge_id)
            .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(time_property).last_n(1))
            .send()
            .await?;
        expect_that!(last.body(), has_evolution("level", len(eq(1))));
    }
    Ok(())
}
