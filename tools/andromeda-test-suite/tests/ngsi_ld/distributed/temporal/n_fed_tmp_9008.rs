//! Source histories a write would refuse are dropped with a warning.

use andromeda_test_suite::{broker, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, NgsiLdWarnings, WarningCode},
    model::NgsiLdMediaType,
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities over an inclusive registration whose source answers one admissible
/// history, one with an instance whose `unitCode` is no common code, one at latitude 91 and one at longitude 181
/// answers 200 with the admissible one alone and `111` NGSILD-Warnings
/// Clause: 4.7, Table 5.2.5-1, 5.7.4.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9008_source_histories_a_write_would_refuse_are_dropped_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let probe_type = namespace.entity_type("RevalidatedProbe")?;
    let registration_id = namespace.registration_id("history")?;
    let valid_id = namespace.urn("RevalidatedProbe", "valid")?;
    // The source answers histories a write would refuse, which the model cannot build, so the answer is written as text.
    source
        .answer(
            Method::GET,
            "/history/ngsi-ld/v1/temporal/entities",
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-valid", "type": "RevalidatedProbe_{namespace}", "speed": [{{"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"}}]}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-not-a-code", "type": "RevalidatedProbe_{namespace}", "speed": [{{"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"}}, {{"type": "Property", "value": 51, "unitCode": "km/h", "observedAt": "2020-01-02T00:00:00Z"}}]}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-past-pole", "type": "RevalidatedProbe_{namespace}", "location": [{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [0, 91]}}, "observedAt": "2020-01-01T00:00:00Z"}}]}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-past-antimeridian", "type": "RevalidatedProbe_{namespace}", "location": [{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [181, 0]}}, "observedAt": "2020-01-01T00:00:00Z"}}]}}]"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("history")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&probe_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&valid_id)));
    // Clause 6.3.17 fixes the code of the warning and leaves its text and the number of warnings to the broker.
    let warnings = page.warnings().valid().map(NgsiLdWarnings::as_slice).unwrap_or_default();
    expect_that!(warnings, not(is_empty()));
    expect_that!(
        warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>(),
        each(eq(&WarningCode::RevalidationFailed))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
