//! A deletion records a tombstone and no null observation.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, NgsiLdMediaType, Property, Relationship},
    query::temporal_query::TemporalProperty,
};

/// Pins: After Partial Attribute Updates removing a unitCode and then deleting the attribute, its Temporal Evolution
/// read on observedAt holds the two observations of 4 and none whose unitCode or value is urn:ngsi-ld:null, and read on
/// deletedAt holds the one instance whose value is urn:ngsi-ld:null
/// Clause: 4.5.0, 4.5.7, 5.5.8, 4.11, Table 5.2.21-1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9008_a_deletion_records_a_tombstone_and_no_null_observation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_temporal_consequence_id = namespace.urn("Building", "temporal-consequence")?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_temporal_consequence = Entity::builder()
        .id(building_temporal_consequence_id.clone())
        .entity_type(building_type)
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", Relationship::builder(sensor_id)),
        )
        .build()?;
    broker.entities().create(&building_temporal_consequence).send().await?.done()?;
    // The Property builder refuses the NGSI-LD Null, so both Partial Attribute Updates are sent as written.
    for body in [
        r#"{"type": "Property", "value": 4, "unitCode": "urn:ngsi-ld:null"}"#,
        r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#,
    ] {
        let request = json_request_to(
            &broker,
            Method::PATCH,
            &format!("entities/{building_temporal_consequence_id}/attrs/airQualityLevel"),
            body,
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    }
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client.temporal().retrieve(&building_temporal_consequence_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "airQualityLevel",
            all!(
                len(eq(2)),
                nth(0, any_attribute().value(4)),
                nth(1, any_attribute().value(4)),
                not(contains(any_attribute().null_value()))
            )
        )
    );
    let deleted = client
        .temporal()
        .retrieve(&building_temporal_consequence_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    expect_that!(
        deleted.body(),
        has_evolution(
            "airQualityLevel",
            all!(len(eq(1)), nth(0, all!(attribute_of(AttributeKind::Property).null_value(), deleted_instance())))
        )
    );
    Ok(())
}
