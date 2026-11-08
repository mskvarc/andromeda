//! The Location of a Context Source Registration names its id percent-encoded as one path segment.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    followed_location::{identifier, request},
    namespace::Namespace,
    problem::answered,
    segment_identifier::segment_identifiers,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::registration::{CSourceRegistration, RegistrationId, RegistrationInfo};

/// Pins: Register Context Source with an id holding /, ?, #, %41 or a non-ASCII character answers a Location ending
/// in /ngsi-ld/v1/csourceRegistrations/ and the id percent-encoded as one path segment, and a GET on that Location
/// answers the Context Source Registration with that id
/// Clause: 6.8.3.1, 6.9.3.1
/// Recorded: IETF RFC 3986 section 3.3 admits only pchar in a path segment, so the relative path names the created
/// Context Source Registration only with every other octet of its id percent-encoded, % included
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9134_the_location_of_a_registration_names_its_id_percent_encoded_as_one_path_segment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let bus_type = namespace.entity_type("Bus")?;
    for segment_identifier in segment_identifiers(&namespace, "ContextSourceRegistration") {
        let registration_id = RegistrationId::verbatim(&segment_identifier.id)?;
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([bus_type.as_str()]).build()?])
            .build()?;
        let created = broker.with_responses().registrations().register(&registration).send().await?;
        expect_that!(
            header_value(created.head.headers(), &LOCATION),
            some(ends_with(format!("/ngsi-ld/v1/csourceRegistrations/{}", segment_identifier.segment)))
        );
        let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        expect_that!(identifier(&answer?)?, eq(registration_id.as_uri()));
        broker.registrations().delete(&registration_id).send().await?;
    }
    Ok(())
}
