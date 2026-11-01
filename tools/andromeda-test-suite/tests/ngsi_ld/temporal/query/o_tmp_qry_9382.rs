//! A deleted Entity whose Temporal Evolution has an instance in the window is selected on every axis it reaches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query::{self, TemporalProperty},
};

/// The axes each window is read on.
const AXES: [TemporalProperty; 4] = [
    TemporalProperty::ObservedAt,
    TemporalProperty::CreatedAt,
    TemporalProperty::ModifiedAt,
    TemporalProperty::DeletedAt,
];

/// Pins: after Delete Entity of an Entity created with one speed instance observed at 2021-01-01, Query Temporal
/// Evolution of Entities by its type selects it in timerel=after 2020-01-01 and between 2020-01-01 and 2090-01-01
/// on observedAt, createdAt and modifiedAt, and selects nothing in after 2090-01-01 or between 2090-01-01 and
/// 2100-01-01 on any of observedAt, createdAt, modifiedAt and deletedAt
/// Clause: 4.8, 4.11, 5.6.6, 5.7.4.4, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default and keeps the Temporal Evolution of a deleted
/// Entity
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9382_a_deleted_entity_with_an_instance_in_the_window_is_selected_on_every_axis_it_reaches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", Property::builder(30).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    broker.entities().delete(&sensor_id).send().await?.done()?;
    let (near, far, end) = (
        "2020-01-01T00:00:00Z".parse()?,
        "2090-01-01T00:00:00Z".parse()?,
        "2100-01-01T00:00:00Z".parse()?,
    );
    // The instance is observed at 2021-01-01 and created and last modified at the instant it is recorded, which lies
    // between 2020 and 2090 (clause 4.8); any instant the deletion records lies there too.
    let windows = [
        (temporal_query::after(near), 1, &AXES[..3]),
        (temporal_query::between(near, far), 1, &AXES[..3]),
        (temporal_query::after(far), 0, &AXES[..]),
        (temporal_query::between(far, end), 0, &AXES[..]),
    ];
    for (window, count, axes) in windows {
        for property in axes {
            // clone: every axis reads the same window
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .temporal()
                .query(window.clone().time_property(*property))
                .entity_type(&sensor_type)
                .send()
                .await?;
            expect_that!(page.items(), len(eq(count)), "{property}");
            expect_that!(page.items(), each(has_id(&sensor_id)), "{property}");
        }
    }
    Ok(())
}
