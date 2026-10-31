//! An inline join answer of a source with another context is read in the client terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, lacks_attribute},
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    remote_context::RemoteContexts,
    source_answer,
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType, OneOrMany, Output, Relationship},
    query::Join,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with join inline answered by a source in its configured context nests the Linked Entity
/// under its Relationship, and both the root and the nested copy are read in the client's terms
/// Clause: 4.5.23.2, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_fed_qry_6608_an_inline_join_answer_of_a_source_with_another_context_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_remote8_id = namespace.urn("Vehicle", "remote8")?;
    let vehicle_linked8_id = namespace.urn("Vehicle", "linked8")?;
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
    let nested = Entity::projected_output_builder()
        .id(vehicle_linked8_id.clone())
        .entity_type(car_type.clone())
        .property("velocity", 7)
        .build()?;
    let next = Relationship::<Output>::output_builder(Relationship::builder(vehicle_linked8_id.clone()))
        .entity(OneOrMany::one(nested))
        .build()?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_remote8_id.clone())
        .entity_type(car_type)
        .property("velocity", 3)
        .output_attribute("next_8", next)
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .context_source_info(("jsonldContext", configured.as_str()))
        .build()?;
    linked.registrations().register(&registration).send().await?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let remote8 = with_id(page.items(), &vehicle_remote8_id).or_fail()?;
    expect_that!(remote8, has_type(&veh_type));
    let nested_entities = linked_entities(remote8, &["next_8"]);
    expect_that!(nested_entities, len(eq(1)));
    expect_that!(nested_entities, nth(0, has_type(&veh_type)));
    expect_that!(nested_entities, nth(0, has_attribute("spd", any_attribute().value(7))));
    expect_that!(nested_entities, nth(0, lacks_attribute("velocity")));
    expect_that!(with_id(page.items(), &vehicle_linked8_id), none());
    Ok(())
}
