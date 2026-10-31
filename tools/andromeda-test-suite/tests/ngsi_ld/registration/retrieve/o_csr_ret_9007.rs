//! A registration location reads back as its nearest double.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    geometry_matcher::geometry,
    namespace::Namespace,
    problem::answered,
    raw_answer::send_raw,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson, Value},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
};

/// Pins: A registration whose location is the Point `[13.123456789012345678, 52.123456789012345678]` reads back
/// that location as `[13.123456789012346,52.123456789012344]`, not cut to nine decimal places
/// Clause: 4.7.2, 5.2.9, 5.9.3
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it, a registration's geometries as an entity's, and writes it in the shortest spelling
/// of that double
#[gtest]
#[tokio::test]
async fn o_csr_ret_9007_a_registration_location_reads_back_as_its_nearest_double() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = format!("urn:ngsi-ld:ContextSourceRegistration:{namespace}").parse::<RegistrationId>()?;
    let tree_type = namespace.entity_type("Tree")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(tree_type)]).build()?])
        .build()?;
    // The ordinates carry eighteen decimals, which a double cannot hold, so `location` is written as text.
    let location: RawJson = r#"{"type": "Point", "coordinates": [13.123456789012345678, 52.123456789012345678]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The ordinates are pinned as the broker writes their digits, which the typed geometry reads into numbers, so the
    // answer is also read as received.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(
        String::from_utf8_lossy(response.body()).into_owned(),
        contains_substring(r#""coordinates":[13.123456789012346,52.123456789012344]"#)
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        retrieved.location(),
        some(
            geometry()
                .of_type("Point")
                .ordinate([0], Value::Number("13.123456789012346".parse()?))
                .ordinate([1], Value::Number("52.123456789012344".parse()?))
        )
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
