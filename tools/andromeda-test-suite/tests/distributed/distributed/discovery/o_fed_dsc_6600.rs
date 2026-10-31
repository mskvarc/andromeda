//! A type listing answered in a configured context is read in the client terms.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    remote_context::RemoteContexts,
    source_discovery,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, NgsiLdMediaType, Uri, discovery::EntityTypeList},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Available Entity Types answered by a source in its registration's `jsonldContext` lists its type
/// as the client's term and not as the source's
/// Clause: 5.7.6, 4.3.6.6
/// Recorded: clause 5.7.11 lets type discovery be answered locally or forwarded to the sources supporting it;
/// Andromeda forwards it and reads the answer in the client's context
#[gtest]
#[tokio::test]
async fn o_fed_dsc_6600_a_type_listing_answered_in_a_configured_context_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let car_type = namespace.entity_type("Car")?;
    let list_id: Uri = format!("urn:ngsi-ld:EntityTypeList:{namespace}-1").parse()?;
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
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/ngsi-ld/v1/types").priority(5),
            source_discovery::entity_type_list(&EntityTypeList::new(list_id.clone(), Vec::new())),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/ngsi-ld/v1/types")
                .header(HeaderCondition::Contains(HeaderName::from_static("link"), configured.to_string()))
                .priority(1),
            source_discovery::entity_type_list(&EntityTypeList::new(list_id, vec![car_type.clone()])),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntityTypes])
        .context_source_info(("jsonldContext", configured.as_str()))
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let types = linked.with_accept(Accept::new(NgsiLdMediaType::Json)).entity_types().list().send().await?;
    expect_that!(types.type_list, contains(eq(&veh_type)));
    expect_that!(types.type_list, not(contains(eq(&car_type))));
    Ok(())
}
