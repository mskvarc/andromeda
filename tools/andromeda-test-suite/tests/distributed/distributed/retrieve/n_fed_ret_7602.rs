//! Attrs is expanded with the request context before the registration is matched.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    core_context::core_entry,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A with attrs speed under the @context that maps speed to the IRI an exclusive
/// registration at broker B names answers 200 with broker B's speed, and the same attrs under the default
/// vocabulary, another IRI, matches no registration and answers 404
/// Clause: 5.5.7, 5.12, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_ret_7602_attrs_is_expanded_with_the_request_context_before_the_registration_is_matched() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker_b, &context).await?;
    let body_context_b = broker_b.with_context(context.clone())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let body_context = broker.with_context(context)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    body_context_b.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    let retrieved = linked
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["speed".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
