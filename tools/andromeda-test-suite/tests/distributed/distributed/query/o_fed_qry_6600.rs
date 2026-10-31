//! A client term is forwarded in the spelling of the context the request named.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities naming a context that aliases an attribute and a type is forwarded with those aliases and
/// never the last segment of their IRIs, and Query Entities under the core context is forwarded with its plain
/// terms
/// Clause: 4.3.6.1, 4.3.6.3, 5.7.2.4
/// Recorded: the spec does not say which @context a forwarded request is compacted against when the registration
/// names no `jsonldContext`; Andromeda compacts against the single absolute @context the client request named, else
/// the core @context
#[gtest]
#[tokio::test]
async fn o_fed_qry_6600_a_client_term_is_forwarded_in_the_spelling_of_the_context_the_request_named() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [];
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities")
        .query("type", format!("Veh_{namespace}"))
        .query("attrs", "spd");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    let forwarded_request_2 = SourceRequest::new(Method::GET, "/b/ngsi-ld/v1/entities")
        .query("type", format!("Vehicle_{namespace}"))
        .query("attrs", "speed");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
