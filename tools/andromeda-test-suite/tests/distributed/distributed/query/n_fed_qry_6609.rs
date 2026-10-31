//! An answer entity with its own context is read in that context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_lookup::with_id,
    entity_matcher::has_attribute,
    forwarded_head,
    namespace::Namespace,
    remote_context::RemoteContexts,
    source_ld_json,
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::Method;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Context, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities answered by a source with an entity carrying its own @context, which aliases its terms to
/// the client's IRIs, reaches the client in the client's terms
/// Clause: 4.3.6.6, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_6609_an_answer_entity_with_its_own_context_is_read_in_that_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let client_context = contexts.url("jc/client.jsonld")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_remote9_id = namespace.urn("Vehicle", "remote9")?;
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
    let source_entities = [Entity::output_builder()
        .id(vehicle_remote9_id.clone())
        .entity_type(car_type)
        .property("velocity", 3)
        .context(Context::Single(contexts.reference("jc/configured.jsonld")?))
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_ld_json::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    let remote9 = with_id(page.items(), &vehicle_remote9_id).or_fail()?;
    expect_that!(remote9, has_type(&veh_type));
    expect_that!(remote9, has_attribute("spd", any_attribute().value(3)));
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let client_link = Link::try_json_ld_context(IriRefBuf::new(client_context.to_string()).ok().or_fail()?)?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&client_link)));
    Ok(())
}
