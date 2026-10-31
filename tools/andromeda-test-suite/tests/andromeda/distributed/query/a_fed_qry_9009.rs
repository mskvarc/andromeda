//! A revalidation warning names the registration and not what the source sent.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: The source entities dropped by Query Entities are reported in one NGSILD-Warning with code 111 naming the registration
/// and counting the three dropped entities, which repeats neither the dropped entities nor their refused members
/// Clause: 6.3.17
/// Recorded: 6.3.17 fixes the warning code and leaves its text to the broker; Andromeda names the registration and
/// counts the entities it left out, and leaves what the source sent to its log
#[gtest]
#[tokio::test]
async fn a_fed_qry_9009_a_revalidation_warning_names_the_registration_and_not_what_the_source_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let probe_type = namespace.entity_type("RevalidatedProbe")?;
    let registration_id = namespace.registration_id("current")?;
    let valid_id = namespace.urn("RevalidatedProbe", "valid")?;
    // The source answers entities the model refuses (a unit that is no common code, a unit on a Relationship, an open
    // ring), so its answer is written as text.
    let answer = format!(
        r#"[
{{"id": "{valid_id}", "type": "{probe_type}", "speed": {{"type": "Property", "value": 50, "unitCode": "KMH"}}}},
{{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-not-a-code", "type": "{probe_type}", "speed": {{"type": "Property", "value": 50, "unitCode": "km/h"}}}},
{{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-unitless", "type": "{probe_type}", "parkedIn": {{"type": "Relationship", "object": "urn:ngsi-ld:Parking:{namespace}-1", "unitCode": "KGM"}}}},
{{"id": "urn:ngsi-ld:RevalidatedProbe:{namespace}-open-ring", "type": "{probe_type}", "location": {{"type": "GeoProperty", "value": {{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}}}}}}
]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/current/ngsi-ld/v1/entities"),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &answer),
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
    let warnings = page.warnings().valid().or_fail()?;
    expect_that!(
        warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>(),
        elements_are![eq(&WarningCode::RevalidationFailed)]
    );
    expect_that!(
        warnings.iter().map(NgsiLdWarning::text).collect::<Vec<_>>(),
        elements_are![all!(
            eq(&format!("3 of the entities context source {registration_id} answered cannot be used")),
            not(contains_substring("not-a-code")),
            not(contains_substring("unitCode"))
        )]
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
