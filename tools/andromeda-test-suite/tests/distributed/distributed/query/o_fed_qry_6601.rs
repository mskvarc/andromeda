//! An exclusive registration supplies its id type and attribute in the spelling of the context.

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

/// Pins: Query Entities by an aliased type alone, over an exclusive registration of one entity and attribute, is
/// forwarded with the registration's id, the aliased type and the aliased attribute
/// Clause: 4.3.6.1, 4.3.6.3, 5.7.2.4
/// Recorded: clause 4.3.6.1 requires the constraints of a registration to be respected; that the forwarded query
/// carries the registered id and attribute, spelt in the client's context, is Andromeda's way of respecting them
#[gtest]
#[tokio::test]
async fn o_fed_qry_6601_an_exclusive_registration_supplies_its_id_type_and_attribute_in_the_spelling_of_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_exclusive_id = namespace.urn("Vehicle", "exclusive")?;
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
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id(vehicle_exclusive_id)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .query("type", format!("Veh_{namespace}"))
        .query("attrs", "spd")
        .query("id", format!("urn:ngsi-ld:Vehicle:{namespace}-exclusive"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
