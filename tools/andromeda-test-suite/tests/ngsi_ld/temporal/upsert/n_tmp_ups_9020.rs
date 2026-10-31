//! A temporal create records a deleted instance with the null and deleted at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    query::temporal_query,
};

/// Pins: Create or Update Temporal Evolution with an instance whose value is urn:ngsi-ld:null, beside an instance with a
/// value, records both: the Temporal Evolution returns the deleted instance with the null value, its observedAt and a
/// deletedAt
/// Clause: 4.5.7, 5.5.4, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9020_a_temporal_create_records_a_deleted_instance_with_the_null_and_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_deleted_instance_id = namespace.urn("Vehicle", "deleted-instance")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let core = config.core_context();
    // A request cannot hold the NGSI-LD Null of a deleted instance, so the Evolution is sent as written.
    let body = format!(
        r#"{{"id": "{vehicle_deleted_instance_id}", "type": "{vehicle_type}", "speed": [{{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}}, {{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2024-01-02T00:00:00Z", "observedAt": "2024-01-02T00:00:00Z"}}], "@context": "{core}"}}"#
    );
    let request = request_to(&broker, Method::POST, "temporal/entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(body));
    let created = broker.send_unchecked(request).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_deleted_instance_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let first = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let second = "2024-01-02T00:00:00Z".parse::<DateTime>()?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(1).observed_at(first)),
                contains(all!(deleted_instance(), any_attribute().observed_at(second)))
            )
        )
    );
    Ok(())
}
