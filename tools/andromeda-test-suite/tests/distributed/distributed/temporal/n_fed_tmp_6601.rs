//! A temporal answer of a configured context is read in the client terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_lookup::with_id,
    evolution_matcher::{has_evolution, lacks_evolution},
    forwarded_head,
    namespace::Namespace,
    nth::nth,
    remote_context::RemoteContexts,
    source_answer,
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::Method;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Context, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities answered by a source in its registration's `jsonldContext` reaches
/// the client in the client's terms and the forwarded request carries one context Link naming that context
/// Clause: 4.3.6.6, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6601_a_temporal_answer_of_a_configured_context_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_remote61_id = namespace.urn("Vehicle", "remote61")?;
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
    let configured = contexts
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
    let source_evolutions = [TemporalEntity::builder()
        .id(vehicle_remote61_id.clone())
        .entity_type(car_type)
        .property("velocity", Property::builder(3).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?];
    source
        .answer(
            Method::GET,
            "/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .context_source_info(("jsonldContext", configured.as_str()))
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    let remote61 = with_id(page.items(), &vehicle_remote61_id).or_fail()?;
    expect_that!(remote61, has_type(&veh_type));
    expect_that!(remote61, has_evolution("spd", nth(0, any_attribute().value(3))));
    expect_that!(remote61, lacks_evolution("velocity"));
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let configured_link = Link::try_json_ld_context(IriRefBuf::new(configured.to_string()).ok().or_fail()?)?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&configured_link)));
    Ok(())
}
