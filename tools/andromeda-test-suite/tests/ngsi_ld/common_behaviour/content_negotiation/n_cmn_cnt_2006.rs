//! Create registration declaring merge patch json is 415.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, wire::content_type};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, ContentType},
        operation::RegisterContextSource,
    },
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration (POST) declaring application/merge-patch+json is refused with a bare
/// 415 and stores nothing
/// Clause: 6.3.4, 5.9.2
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2006_create_registration_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint(&format!("sources/{namespace}"))?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type).with_id(device_id)])
            .build()?])
        .build()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so the media type is replaced.
    let request = content_type(
        broker.unchecked(&RegisterContextSource { registration: &registration })?,
        &ContentType::MergePatch.media_type(),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
