//! A configured jsonld context spells the forwarded query and the answer is read in the client terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_head,
    identity_matcher::has_id,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
    type_matcher::has_type,
    wire::header_values,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Link},
        operation::RegisterContextSource,
    },
    model::{Context, Entity, NgsiLdMediaType, RawJson},
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities forwarded to a registration with a `jsonldContext` carries one context Link naming it and
/// the type, attrs and q in its terms, and the answer spelt in it reaches the client in the client's own terms
/// Clause: 4.3.6.6, 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_qry_6606_a_configured_jsonld_context_spells_the_forwarded_query_and_the_answer_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_remote6_id = namespace.urn("Vehicle", "remote6")?;
    let car_type = namespace.entity_type("Car")?;
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
    let source_entities = [Entity::output_builder()
        .id(vehicle_remote6_id.clone())
        .entity_type(car_type)
        .property("velocity", 3)
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
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
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .q(q::attr("spd").gt(1))
        .send()
        .await?;
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&vehicle_remote6_id),
            has_attribute("spd", any_attribute().value(3)),
            lacks_attribute("velocity"),
            has_type(&veh_type)
        ))
    );
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .query("type", format!("Car_{namespace}"))
        .query("attrs", "velocity")
        .query("q", "velocity>1");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(header_values(forwarded_head::head(forwarded_one).headers(), &LINK), len(eq(1)));
    expect_that!(
        forwarded_head::head(forwarded_one).typed::<Link>().valid(),
        some(eq(&Link::try_json_ld_context(IriRefBuf::new(
            contexts.url("jc/configured.jsonld")?.to_string()
        )?)?))
    );
    Ok(())
}
