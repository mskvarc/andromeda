//! An attribute whose instances were all deleted selects its Entity in no window.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::instance_id_where,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
};

/// The axes each window is read on.
const AXES: [TemporalProperty; 4] = [
    TemporalProperty::ObservedAt,
    TemporalProperty::CreatedAt,
    TemporalProperty::ModifiedAt,
    TemporalProperty::DeletedAt,
];

/// Pins: after Delete Attribute Instance of the one speed instance, observed at 2099-06-01, of an Entity created
/// through Create Temporal Evolution beside a marker instance observed at 2010-01-01, Query Temporal Evolution of
/// Entities by its type with attrs=speed selects nothing in timerel=after 2090-01-01, between 2090-01-01 and
/// 2100-01-01, after 2000-01-01 or between 2000-01-01 and 2090-01-01, on any of observedAt, createdAt, modifiedAt and
/// deletedAt, while the same type with no attrs on createdAt after 2000-01-01 still selects the Entity by its marker
/// Clause: 4.11, 5.6.15, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9381_an_attribute_whose_instances_were_all_deleted_selects_its_entity_in_no_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", Property::builder(30).observed_at("2099-06-01T00:00:00Z".parse()?))
        .property("marker", Property::builder(1).observed_at("2010-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let speed_instance = instance_id_where(retrieved.body(), &"speed".parse()?, any_attribute().value(30))?;
    broker
        .temporal()
        .delete_instance(&sensor_id, &"speed".parse::<AttributeName>()?, &speed_instance)
        .send()
        .await?;
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
            // clone: every axis reads the same window
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .temporal()
                .query(window.clone().time_property(property))
                .entity_type(&sensor_type)
                .attrs(vec!["speed".parse()?])
                .send()
                .await?;
            expect_that!(page.items(), len(eq(0)), "{property}");
        }
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after(past).time_property(TemporalProperty::CreatedAt))
        .entity_type(&sensor_type)
        .send()
        .await?;
    expect_that!(page.items(), all!(len(eq(1)), each(has_id(&sensor_id))));
    Ok(())
}
