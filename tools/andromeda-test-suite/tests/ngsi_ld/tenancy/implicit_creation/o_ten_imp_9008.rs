//! A create after a refused one creates the tenant.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdTenant},
        operation::CreateEntity,
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, Tenant},
};

/// Pins: Create Entity on a new Tenant refused with 400 `BadRequestData`, then the same Create Entity with a valid
/// `observedAt` there answers 201 and creates the Tenant: the Entity then reads back there
/// Clause: 5.5.10, 5.6.1, 5.5.4, 6.3.14
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9008_a_create_after_a_refused_one_creates_the_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let new_tenant = broker.with_tenant(format!("{namespace}-new").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 55, "observedAt": "2024-01-01T00:00:00.1234567Z"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let request = new_tenant.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = new_tenant.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(55).observed_at("2024-01-01T00:00:00.123456Z".parse()?))
        .build()?;
    let received = new_tenant.with_responses().entities().create(&vehicle_2).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&format!("{namespace}-new").parse::<Tenant>()?))
    );
    let headed = new_tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&format!("{namespace}-new").parse::<Tenant>()?))
    );
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
