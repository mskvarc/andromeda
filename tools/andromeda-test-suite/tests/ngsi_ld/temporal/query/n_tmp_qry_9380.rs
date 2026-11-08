//! An Entity whose Temporal Evolution was deleted is selected in no window.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
};

/// The axes each window is read on.
const AXES: [TemporalProperty; 4] = [
    TemporalProperty::ObservedAt,
    TemporalProperty::CreatedAt,
    TemporalProperty::ModifiedAt,
    TemporalProperty::DeletedAt,
];

/// Pins: after Delete Temporal Evolution of an Entity created through Create Temporal Evolution with one speed
/// instance observed at 2099-06-01, Query Temporal Evolution of Entities by its type selects nothing in timerel=after
/// 2090-01-01, between 2090-01-01 and 2100-01-01, after 2000-01-01 or between 2000-01-01 and 2090-01-01, on any of
/// observedAt, createdAt, modifiedAt and deletedAt
/// Clause: 4.11, 5.6.16, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9380_an_entity_whose_temporal_evolution_was_deleted_is_selected_in_no_window() -> Result<()> {
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
    broker.temporal().delete(&sensor_id).send().await?;
    let (past, far, end) = (
        "2000-01-01T00:00:00Z".parse()?,
        "2090-01-01T00:00:00Z".parse()?,
        "2100-01-01T00:00:00Z".parse()?,
    );
    let windows = [
        temporal_query::after(far),
        temporal_query::between(far, end),
        temporal_query::after(past),
        temporal_query::between(past, far),
    ];
    for window in windows {
        for property in AXES {
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .temporal()
                .query(window.clone().time_property(property))
                .entity_type(&sensor_type)
                .send()
                .await?;
            expect_that!(page.items(), len(eq(0)), "{property}");
        }
    }
    Ok(())
}
