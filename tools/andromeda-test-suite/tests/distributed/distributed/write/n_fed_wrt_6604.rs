//! A forwarded update result is read back in full names.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::json_equals,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    nth::nth,
    partial_outcome::PartialSuccess,
    remote_context::RemoteContexts,
    report_matcher::not_updated,
    source_answer,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    model::{Context, EntityFragment},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes forwarded to an exclusive registration with a `jsonldContext`, whose source
/// answers 207 naming the attribute in that context, answers 207 naming it by its full IRI
/// Clause: 6.3.6, 4.3.6.6, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6604_a_forwarded_update_result_is_read_back_in_full_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_write5_id = namespace.urn("Vehicle", "write5")?;
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
    // The update result names the attribute by a term of the registration's context, which the source answer helpers do
    // not write for an update result, so the answer is written as text.
    source
        .answer(
            Method::POST,
            &format!("/ngsi-ld/v1/entities/{vehicle_write5_id}/attrs"),
            source_answer::text(
                StatusCode::MULTI_STATUS,
                HeaderValue::from_static("application/json"),
                r#"{"updated": [], "notUpdated": [{"attributeName": "velocity", "reason": "the source already holds it"}]}"#,
            ),
        )
        .await?;
    let endpoint = contexts.url("jc/configured.jsonld")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id(vehicle_write5_id.clone())])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::AppendAttrs, DistributedOperation::RetrieveEntity])
        .context_source_info(("jsonldContext", endpoint.as_str()))
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let fragment = EntityFragment::builder().property("spd", 9).build()?;
    let received = linked
        .with_responses()
        .entities()
        .append_attributes(&vehicle_write5_id, &fragment)
        .send()
        .await?;
    let report = received.outcome.partial()?;
    expect_that!(&report.not_updated, len(eq(1)));
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    // The attribute name is pinned as the report spells it, so the body is read as received.
    expect_that!(
        &received.body,
        json_equals("$.notUpdated[0].attributeName", format!("https://test.andromeda/{namespace}/vocab/speed"))
    );
    expect_that!(&received.body, not(json_equals("$.notUpdated[0].attributeName", "velocity")));
    let forwarded = source
        .received_on(Method::POST, &format!("/ngsi-ld/v1/entities/{vehicle_write5_id}/attrs"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_fragment = forwarded_body::fragment(forwarded_one)?;
    expect_that!(&forwarded_fragment, has_attribute("velocity", any_attribute()));
    Ok(())
}
