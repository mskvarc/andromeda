//! An instance observed after it was recorded is selected on every axis it reaches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
};

/// The axes each window is read on, in the order of the expected counts.
const AXES: [TemporalProperty; 4] = [
    TemporalProperty::ObservedAt,
    TemporalProperty::CreatedAt,
    TemporalProperty::ModifiedAt,
    TemporalProperty::DeletedAt,
];

/// Pins: the one speed instance of an Entity created through Create Temporal Evolution with observedAt
/// 2099-06-01, recorded now, selects the Entity by type on observedAt in timerel=after 2090-01-01, between
/// 2090-01-01 and 2100-01-01 and after 2000-01-01; on createdAt and modifiedAt in after 2000-01-01 and between
/// 2000-01-01 and 2090-01-01; and in no window on deletedAt
/// Clause: 4.8, 4.11, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9379_an_instance_observed_after_it_was_recorded_is_selected_on_every_axis_it_reaches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", Property::builder(30).observed_at("2099-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor).send().await?.created()?;
    let (past, far, end) = (
        "2000-01-01T00:00:00Z".parse()?,
        "2090-01-01T00:00:00Z".parse()?,
        "2100-01-01T00:00:00Z".parse()?,
    );
    // The instance is observed at 2099-06-01, and created, last modified and never deleted at the instant it is
    // recorded, which lies between 2000 and 2090 (clause 4.8).
    let windows = [
        (temporal_query::after(far), [1, 0, 0, 0]),
        (temporal_query::between(far, end), [1, 0, 0, 0]),
        (temporal_query::after(past), [1, 1, 1, 0]),
        (temporal_query::between(past, far), [0, 1, 1, 0]),
    ];
    for (window, counts) in windows {
        for (property, count) in AXES.into_iter().zip(counts) {
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .temporal()
                .query(window.clone().time_property(property))
                .entity_type(&sensor_type)
                .send()
                .await?;
            expect_that!(page.items(), len(eq(count)), "{property}");
            expect_that!(page.items(), each(has_id(&sensor_id)), "{property}");
        }
    }
    Ok(())
}
