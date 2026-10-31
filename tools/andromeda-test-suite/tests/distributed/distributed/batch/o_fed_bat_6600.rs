//! A mixed context batch is forwarded with fully qualified names and no item context.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals, json_exists},
    broker,
    context_source::ContextSource,
    core_context::core_context_link,
    forwarded_head,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Link, operation::BatchCreate},
    model::{Context, Entity, Lossless},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use wiremock::ResponseTemplate;

/// Pins: Batch Entity Creation whose two items name two different contexts is forwarded as one JSON request to a
/// `createBatch` source with no item @context, no context other than the core and each item's type and attribute
/// fully qualified
/// Clause: 5.6.7, 6.3.5, 4.3.6.6
/// Recorded: the spec does not say which @context a forwarded request is compacted against when the registration
/// names no `jsonldContext`; Andromeda compacts against the single absolute @context the client request named, else
/// the core @context
#[gtest]
#[tokio::test]
async fn o_fed_bat_6600_a_mixed_context_batch_is_forwarded_with_fully_qualified_names_and_no_item_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let client_context = contexts.url("jc/client.jsonld")?;
    let rate_context = contexts.url("jc/rate.jsonld")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let batch = linked.with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_batch1_id = namespace.urn("Vehicle", "batch1")?;
    let vehicle_batch2_id = namespace.urn("Vehicle", "batch2")?;
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
            "jc/rate.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "rate": "https://test.andromeda/{namespace}/vocab/speed",
        "Veh_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/ngsi-ld/v1/entityOperations/create",
            ResponseTemplate::new(StatusCode::CREATED.as_u16()).insert_header(CONTENT_TYPE, HeaderValue::from_static("application/json")),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateBatch])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let entities = [
        Entity::builder()
            .id(vehicle_batch1_id)
            .entity_type(veh_type.clone())
            .property("spd", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_batch2_id)
            .entity_type(veh_type.clone())
            .property("rate", 2)
            .build()?,
    ];
    // The client writes one @context into every document of a batch, so each item carries its own as text.
    let first_document = entities
        .first()
        .map(|entity| UncheckedDocument::from_entity(entity, Lossless::Normalized).set_text("@context", client_context.as_str()))
        .or_fail()?;
    let second_document = entities
        .get(1)
        .map(|entity| UncheckedDocument::from_entity(entity, Lossless::Normalized).set_text("@context", rate_context.as_str()))
        .or_fail()?;
    let request = batch
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[first_document, second_document]);
    let answer = batch.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let forwarded = source.received_on(Method::POST, "/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(1)));
    // The forwarded batch is read as the source received it: the `@context` its items leave out and the expanded
    // names are the bytes under test.
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&core_context_link()?)));
    expect_that!(forwarded_one, json_count("$", 2));
    expect_that!(forwarded_one, json_absent("$[0]['@context']"));
    expect_that!(forwarded_one, json_absent("$[1]['@context']"));
    let vehicle_class = format!("https://test.andromeda/{namespace}/vocab/Vehicle");
    expect_that!(forwarded_one, json_equals("$[0].type", vehicle_class.as_str()));
    expect_that!(forwarded_one, json_equals("$[1].type", vehicle_class.as_str()));
    let speed_term = format!("https://test.andromeda/{namespace}/vocab/speed");
    expect_that!(forwarded_one, json_exists(&format!("$[0]['{speed_term}']")));
    expect_that!(forwarded_one, json_exists(&format!("$[1]['{speed_term}']")));
    Ok(())
}
