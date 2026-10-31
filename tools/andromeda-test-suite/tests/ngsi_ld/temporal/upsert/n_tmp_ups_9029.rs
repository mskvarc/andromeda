//! A temporal evolution spelled with at id and at type is created.

use andromeda_test_suite::{attribute_matcher::property, broker, evolution_matcher::has_evolution, namespace::Namespace, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, OneOrMany},
};

/// Pins: Create or Update Temporal Evolution whose id, type and instance types are written as @id and @type answers 201,
/// and both instances read back as Properties
/// Clause: 4.4, 5.6.11
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9029_a_temporal_evolution_spelled_with_at_id_and_at_type_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_temporal_id = namespace.urn("Building", "temporal")?;
    let building_type = namespace.entity_type("Building")?;
    // The model writes `id` and `type` and refuses their JSON-LD keyword aliases, so the Evolution is sent as written.
    let body = format!(
        r#"{{"@id": "{building_temporal_id}", "@type": "{building_type}", "height": [{{"@type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z"}}, {{"@type": "Property", "value": 2, "observedAt": "2020-01-02T00:00:00Z"}}]}}"#
    );
    let request = json_request_to(&broker, Method::POST, "temporal/entities", &body)?;
    let created = broker.send_unchecked(request).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_temporal_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&building_temporal_id));
    expect_that!(&evolution.entity_type, eq(&OneOrMany::one(building_type)));
    let first = "2020-01-01T00:00:00Z".parse::<DateTime>()?;
    let second = "2020-01-02T00:00:00Z".parse::<DateTime>()?;
    expect_that!(
        evolution,
        has_evolution(
            "height",
            all!(
                len(eq(2)),
                contains(property().value(1).observed_at(first)),
                contains(property().value(2).observed_at(second))
            )
        )
    );
    Ok(())
}
