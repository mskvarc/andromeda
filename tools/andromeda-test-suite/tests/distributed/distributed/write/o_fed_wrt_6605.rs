//! The attribute segment of a forwarded path is spelled in the configured context.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{AttributeName, Context, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Attribute of spd forwarded to an exclusive registration with a `jsonldContext` reaches the source
/// as a DELETE of /attrs/velocity
/// Clause: 4.3.6.6, 5.6.5
/// Recorded: clause 4.3.6.6 names the payload and the query parameters for the compaction under a `jsonldContext`,
/// not the path; Andromeda spells the attribute segment of the path in that context too
#[gtest]
#[tokio::test]
async fn o_fed_wrt_6605_the_attribute_segment_of_a_forwarded_path_is_spelled_in_the_configured_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_write6_id = namespace.urn("Vehicle", "write6")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    contexts
        .serve(
            "jc/client.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "spd": "https://test.andromeda/{namespace}/vocab/speed",
        "Veh_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/configured.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "velocity": "https://test.andromeda/{namespace}/vocab/speed",
        "Car_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    source
        .answer(
            Method::DELETE,
            &format!("/ngsi-ld/v1/entities/{vehicle_write6_id}/attrs/velocity"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type).with_id(vehicle_write6_id.clone())])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::DeleteAttrs, DistributedOperation::RetrieveEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let endpoint = contexts.url("jc/configured.jsonld")?;
    let context_source_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{endpoint}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = linked.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    linked
        .entities()
        .delete_attribute(&vehicle_write6_id, &"spd".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let forwarded = source
        .received_on(Method::DELETE, &format!("/ngsi-ld/v1/entities/{vehicle_write6_id}/attrs/velocity"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
