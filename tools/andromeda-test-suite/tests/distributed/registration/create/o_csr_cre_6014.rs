//! An unknown management member is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose management holds a member Table 5.2.34-1 does not define answers
/// 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.34
/// Recorded: Table 5.2.34-1 lists the supported members and is silent on others; Andromeda refuses a member outside
/// the table
#[gtest]
#[tokio::test]
async fn o_csr_cre_6014_an_unknown_management_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (member `managementInterval` is not allowed here at management), so `management` is written as text.
    let management: RawJson = r#"{"managementInterval": {"startAt": "2020-01-01T00:00:00Z"}}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
