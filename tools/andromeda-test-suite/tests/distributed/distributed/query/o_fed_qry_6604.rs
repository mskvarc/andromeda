//! A compound body context is forwarded against the core and the answer is read in the client terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::{json_absent, json_equals},
    broker,
    context_source::ContextSource,
    core_context::core_context_link,
    entity_lookup::with_id,
    entity_matcher::has_attribute,
    forwarded_head,
    namespace::Namespace,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Link},
        operation::QueryMethod,
    },
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: A posted Query whose @context lists two documents is forwarded to a `queryBatch` source with fully
/// qualified type and attrs, no inline @context and no context other than the core, and the source's fully
/// qualified answer reaches the client in the client's terms
/// Clause: 4.3.6.6, 6.3.5, 5.7.2.4
/// Recorded: the spec does not say which @context a forwarded request is compacted against when the registration
/// names no `jsonldContext`; Andromeda compacts against the single absolute @context the client request named, else
/// the core @context
#[gtest]
#[tokio::test]
async fn o_fed_qry_6604_a_compound_body_context_is_forwarded_against_the_core_and_the_answer_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let posted = broker
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/client.jsonld")?,
            contexts.reference("jc/brand.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_remote4_id = namespace.urn("Vehicle", "remote4")?;
    let vehicle_class = format!("https://test.andromeda/{namespace}/vocab/Vehicle");
    let speed_term = format!("https://test.andromeda/{namespace}/vocab/speed");
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
            "jc/brand.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "brand": "https://test.andromeda/{namespace}/vocab/brand"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_remote4_id.clone())
        .entity_type(vehicle_class.parse::<EntityTypeName>()?)
        .property(speed_term.as_str(), 5)
        .build()?];
    source
        .answer(Method::POST, "/ngsi-ld/v1/entityOperations/query", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let page = posted
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    let remote4 = with_id(page.items(), &vehicle_remote4_id).or_fail()?;
    expect_that!(remote4, has_type(&veh_type));
    expect_that!(remote4, has_attribute("spd", any_attribute().value(5)));
    let forwarded_request = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entityOperations/query").without_query("offset");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    // The forwarded Query document is read as the source received it: the `@context` it leaves out and the expanded
    // names are the bytes under test, and the model keeps the selector members private.
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_one, json_equals("$.entities[0].type", vehicle_class.as_str()));
    expect_that!(forwarded_one, json_equals("$.attrs[0]", speed_term.as_str()));
    expect_that!(forwarded_one, json_absent("$['@context']"));
    expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&core_context_link()?)));
    Ok(())
}
