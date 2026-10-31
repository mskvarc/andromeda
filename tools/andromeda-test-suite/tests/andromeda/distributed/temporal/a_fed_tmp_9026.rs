//! A source window that does not advance is an incomplete answer.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, source_request::SourceRequest, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};
use wiremock::ResponseTemplate;

/// Pins: Query Temporal Evolution of Entities with splitEntities true as aggregatedValues totalCount and a q over a
/// source answering every request 206 with the same Content-Range, so that its window never advances, answers with
/// the instances read, a 111 NGSILD-Warning naming the registration and the cut-short history, and no error
/// Clause: 5.7.4.4, 6.3.10, 6.3.17
/// Recorded: 6.3.17 fixes the warning code and leaves its text to the broker; Andromeda names the registration and
/// says the answer is incomplete
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9026_a_source_window_that_does_not_advance_is_an_incomplete_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Vehicle:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The source answers instances that carry an instanceId, which the model builds none of, so its answer is text.
    let partial = format!(
        r#"[{{"id": "{vehicle_id}", "type": "{vehicle_type}", "speed": [
{{"type": "Property", "value": 1, "observedAt": "2020-08-01T12:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-40"}},
{{"type": "Property", "value": 2, "observedAt": "2020-08-01T13:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-41"}}
]}}]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities"),
            ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
                .set_body_raw(partial, "application/json")
                .insert_header(CONTENT_RANGE, HeaderValue::from_static("DateTime 2020-08-01T12:00:00Z-2020-08-01T13:00:00Z/4")),
        )
        .await?;
    // The 206 answer of an aggregated read is a representation the client does not read, so it is sent as written.
    let request = request_to(
        &broker,
        Method::GET,
        &format!(
            "temporal/entities?type={vehicle_type}&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&splitEntities=true&format=aggregatedValues&aggrMethods=totalCount&q=speed%3E0"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    let warning = HeaderName::from_static("ngsild-warning");
    expect_that!(
        &answer,
        answered(StatusCode::PARTIAL_CONTENT)
            .with_header(CONTENT_RANGE, "DateTime 2020-01-01T00:00:00Z-2020-08-01T13:00:00Z/*")
            .with_header_containing(warning.clone(), "111")
            .with_header_containing(warning.clone(), registration_id.to_string().as_str())
            .with_header_containing(
                warning,
                "cut its history short at an instant it cannot be resumed past; its answer is incomplete"
            )
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
