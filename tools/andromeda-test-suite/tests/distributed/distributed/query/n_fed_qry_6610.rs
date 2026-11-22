//! A forwarded query carries its pattern bare.

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
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities whose q holds a pattern in quotes, `brand~="^Mer(c|z)edes$";spd>1`, is forwarded to a
/// matching registration with the pattern bare, `brand~=^Mer(c|z)edes$;spd>1`, as the clause 4.9 grammar writes a
/// `RegExp`
/// Clause: 4.3.6.6, 4.9, 6.3.19
/// Recorded: clause 4.9 writes `RegExp` without quotes and does not say where it ends; Andromeda forwards a pattern
/// bare unless the bare form would end it early or drop whitespace at its ends
#[gtest]
#[tokio::test]
async fn n_fed_qry_6610_a_forwarded_query_carries_its_pattern_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    contexts
        .serve(
            "jc/client.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "brand": "https://test.andromeda/{namespace}/vocab/brandName",
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
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["brand", "spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
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
        .q(q::attr("brand").matches("\"^Mer(c|z)edes$\"").and(q::attr("spd").gt(1)))
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .query("type", format!("Veh_{namespace}"))
        .query("q", "brand~=^Mer(c|z)edes$;spd>1");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
