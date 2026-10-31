//! At a trusted edge location and pagination links stay relative.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    link_header::link_target,
    namespace::Namespace,
    problem::answered,
    request_fields::with_fields,
    wire::{header_value, request_to},
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, LOCATION},
};
use ngsi_ld::{
    http::{headers::RelationType, operation::RegisterContextSource},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: On a broker trusting its edge, Create Context Source Registration and a Query Context Source Registrations page of one, sent with X-Forwarded-Proto https, answer a Location and a next Link that are root-relative paths carrying no origin
/// Clause: 6.12.3.1, 5.5.9, 6.3.10
/// Recorded: Andromeda extension, not v1.9.1: from a peer `http.forwarded.trust` believes, the scheme of the apiRoot it mints comes from the Forwarded (IETF RFC 7239) or X-Forwarded-Proto field, and the authority from the host of Forwarded or X-Forwarded-Host, else the request's own
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9008_at_a_trusted_edge_location_and_pagination_links_stay_relative() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Edge)?;
    let config = SuiteConfig::get()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let forwarded = [("X-Forwarded-Proto", "https")];
    let registration_ids = [namespace.registration_id(1)?, namespace.registration_id(2)?];
    for registration_id in &registration_ids {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
                .build()?])
            .build()?;
        let request = with_fields(broker.unchecked(&RegisterContextSource { registration: &registration })?, &forwarded)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::CREATED));
        let response = answer?;
        expect_that!(
            header_value(response.headers(), &LOCATION),
            some(starts_with("/ngsi-ld/v1/csourceRegistrations/"))
        );
    }
    // The forwarding fields are no header the typed client writes, so the page query is sent with them as written.
    let query = with_fields(
        request_to(&broker, Method::GET, &format!("csourceRegistrations?type={vehicle_type}&limit=1"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json")),
        &forwarded,
    )?;
    let answer_2 = broker.send_unchecked(query).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    expect_that!(
        link_target(answer_2?.headers(), &RelationType::Next)?,
        some(starts_with("/ngsi-ld/v1/csourceRegistrations"))
    );
    for registration_id in &registration_ids {
        broker.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
