//! A tenant key in a context source info update is stored.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::RetrieveRegistration,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};
use serde_json::{Value, json};

/// Pins: Update Context Source Registration with a `contextSourceInfo` holding an `NGSILD-Tenant` entry answers 204
/// and replaces the stored `contextSourceInfo`
/// Clause: 4.3.6.5, 5.9.3.4
#[gtest]
#[tokio::test]
async fn n_csr_upd_9399_a_tenant_key_in_a_context_source_info_update_is_stored() -> Result<()> {
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
        .mode(RegistrationMode::Inclusive)
        .context_source_info(("X-Api-Key", "secret"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The model refuses the tenant key (a registration contextSourceInfo), so the update is written as text.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{registration_id}"),
        r#"{"contextSourceInfo": [{"key": "NGSILD-Tenant", "value": "t1"}]}"#,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let read = broker
        .send_unchecked(broker.unchecked(&RetrieveRegistration::builder().id(&registration_id).build())?)
        .await;
    verify_that!(&read, answered(StatusCode::OK))?;
    let stored = serde_json::from_slice::<Value>(read?.body())?;
    expect_that!(
        stored.pointer("/contextSourceInfo"),
        some(eq(&json!([{"key": "NGSILD-Tenant", "value": "t1"}])))
    );
    Ok(())
}
