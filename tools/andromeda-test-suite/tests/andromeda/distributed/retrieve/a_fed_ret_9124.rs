//! A flat join answer of a source joins the linked entity without merging it.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_query::query_values,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity with join=flat of a local entity whose inclusive source answers the entity beside a Linked
/// Entity answers a collection of the entity and the Linked Entity, the entity without the Linked Entity's attributes,
/// and no NGSILD-Warning about the source
/// Clause: 4.5.23, 5.7.1.4
/// Recorded: clause 4.5.23 places the Linked Entities of a flat join beside the entity; a source's flat answer is read
/// the same way, its other elements joined to the entity and not merged into it
#[gtest]
#[tokio::test]
async fn a_fed_ret_9124_a_flat_join_answer_of_a_source_joins_the_linked_entity_without_merging_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let person_type = namespace.entity_type("Person")?;
    let car_a = namespace.urn("Car", "a")?;
    let person_1 = namespace.urn("Person", 1)?;
    let registration_id = namespace.registration_id("1")?;
    let car = Entity::builder()
        .id(car_a.clone())
        .entity_type(car_type.clone())
        .property("speed", 50)
        .relationship("owner", person_1.clone())
        .build()?;
    broker.entities().create(&car).send().await?.done()?;
    let path = format!("/current/ngsi-ld/v1/entities/{car_a}");
    // The source's answer is the subject: a flat join of the entity and its Linked Entity, written as the source sends
    // it.
    let answer = format!(
        r#"[
{{"id": "{car_a}", "type": "{car_type}", "colour": {{"type": "Property", "value": "red"}}}},
{{"id": "{person_1}", "type": "{person_type}", "name": {{"type": "Property", "value": "P"}}}}
]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, &path),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &answer),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("current")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(car_type.clone())]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&car_a)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities = retrieved.body().as_slice();
    expect_that!(entities, len(eq(2)));
    let joined_car = with_id(entities, &car_a).or_fail()?;
    expect_that!(joined_car, has_attribute("speed", any_attribute().value(50)));
    expect_that!(joined_car, has_attribute("colour", any_attribute().value("red")));
    expect_that!(joined_car, has_attribute("owner", relationship().object(person_1.clone())));
    expect_that!(joined_car, lacks_attribute("name"));
    let person = with_id(entities, &person_1).or_fail()?;
    expect_that!(person, has_attribute("name", any_attribute().value("P")));
    // The Linked Entity lookup is by identifier, so it reaches every registration of the tenant that serves the type, whichever test
    // registered it. Only a warning about this test's own registration is the subject.
    let warnings_naming_registration = retrieved
        .warnings()
        .valid()
        .into_iter()
        .flat_map(|warnings| warnings.iter().map(|warning| warning.text().to_string()).collect::<Vec<_>>())
        .filter(|text| text.contains(&registration_id.to_string()))
        .collect::<Vec<_>>();
    expect_that!(warnings_naming_registration, is_empty());
    let forwarded = source.received_on(Method::GET, &path).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "join"), elements_are![eq("flat")]);
    broker.registrations().delete(&registration_id).send().await?;
    broker.entities().delete(&car_a).send().await?;
    Ok(())
}
