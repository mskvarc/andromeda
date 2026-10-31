//! A typed retrieve is forwarded with the aliased type and covered attribute.

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
    model::{Context, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity of a registered id with an aliased type, forwarded to a `retrieveEntity` source, carries
/// the aliased type and the covered aliased attribute and never the last segments of their IRIs
/// Clause: 5.7.1.4, 4.3.6.3
/// Recorded: the spec does not say which @context a forwarded request is compacted against when the registration
/// names no `jsonldContext`; Andromeda compacts against the single absolute @context the client request named, else
/// the core @context
#[gtest]
#[tokio::test]
async fn o_fed_ret_6600_a_typed_retrieve_is_forwarded_with_the_aliased_type_and_covered_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_retrieve1_id = namespace.urn("Vehicle", "retrieve1")?;
    let veh_type = namespace.entity_type("Veh")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
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
    let source_entity = Entity::output_builder()
        .id(vehicle_retrieve1_id.clone())
        .entity_type(veh_type.clone())
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/ngsi-ld/v1/entities/{vehicle_retrieve1_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id(vehicle_retrieve1_id.clone())])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_retrieve1_id)
        .entity_type(&veh_type)
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, &format!("/ngsi-ld/v1/entities/{vehicle_retrieve1_id}"))
        .query("type", format!("Veh_{namespace}"))
        .query("attrs", "spd");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
