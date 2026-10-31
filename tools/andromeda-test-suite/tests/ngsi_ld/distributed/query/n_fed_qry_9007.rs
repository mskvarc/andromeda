//! A source entity off the axes is dropped with a warning.

use andromeda_test_suite::{broker, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, NgsiLdWarnings, WarningCode},
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities over an inclusive registration whose source answers one Entity at a valid position and one at
/// latitude 146.05 answers 200 with only the first, and an NGSILD-Warning with code 111
/// Clause: 5.7.2.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_qry_9007_a_source_entity_off_the_axes_is_dropped_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let probe_type = namespace.entity_type("RangeSourceProbe")?;
    let registration_id = namespace.registration_id("source")?;
    let valid_id = namespace.urn("RangeSourceProbe", "valid")?;
    // The source answers entities a write would refuse, which the model cannot build, so the answer is written as text.
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/entities",
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"id": "urn:ngsi-ld:RangeSourceProbe:{namespace}-valid", "type": "RangeSourceProbe_{namespace}", "location": {{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [14.5, 46.05]}}}}}}, {{"id": "urn:ngsi-ld:RangeSourceProbe:{namespace}-invalid", "type": "RangeSourceProbe_{namespace}", "location": {{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [14.5, 146.05]}}}}}}]"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
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
    let warnings = page.warnings().valid().map(NgsiLdWarnings::as_slice).unwrap_or_default();
    expect_that!(
        warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>(),
        contains(eq(&WarningCode::RevalidationFailed))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
