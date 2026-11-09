//! Source entities a write would refuse are dropped with a warning.

use andromeda_test_suite::{broker, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, NgsiLdWarnings, WarningCode},
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities over an inclusive registration whose source answers one admissible Entity, one with a
/// `unitCode` that is no UN/CEFACT common code, one with a `unitCode` on a Relationship and one with an open Polygon
/// ring answers 200 with the admissible one alone and `111` NGSILD-Warnings
/// Clause: 4.5.2, 4.7, Table 5.2.5-1, 5.7.2.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_qry_9008_source_entities_a_write_would_refuse_are_dropped_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let probe_type = namespace.entity_type("RevalidatedProbe")?;
    let registration_id = namespace.registration_id("current")?;
    let valid_id = namespace.urn("RevalidatedProbe", "valid")?;
    // The source answers entities a write would refuse, which the model cannot build, so the answer is written as text.
    source
        .answer(
            Method::GET,
            "/current/ngsi-ld/v1/entities",
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-valid", "type": "RevalidatedProbe_{namespace}", "speed": {{"type": "Property", "value": 50, "unitCode": "KMH"}}}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-not-a-code", "type": "RevalidatedProbe_{namespace}", "speed": {{"type": "Property", "value": 50, "unitCode": "km/h"}}}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-unitless", "type": "RevalidatedProbe_{namespace}", "parkedIn": {{"type": "Relationship", "object": "urn:ngsi-ld:Parking:{namespace}-1", "unitCode": "KGM"}}}}, {{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-open-ring", "type": "RevalidatedProbe_{namespace}", "location": {{"type": "GeoProperty", "value": {{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}}}}}}]"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("current")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&probe_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&valid_id)));
    // Clause 6.3.17 fixes the code of the warning and leaves its text and the number of warnings to the broker.
    let warnings = page.warnings().valid().map_or_default(NgsiLdWarnings::as_slice);
    expect_that!(warnings, not(is_empty()));
    expect_that!(
        warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>(),
        each(eq(&WarningCode::RevalidationFailed))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
