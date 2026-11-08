//! Each creating operation creates the tenant it names.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::problem,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, NgsiLdTenant},
    },
    model::{Entity, ErrorType, NgsiLdMediaType, Property, Tenant, evolution::TemporalEntity},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Entity, Batch Entity Creation, Create or Update Temporal Evolution, Create Subscription, Register
/// Context Source and Create Context Source Registration Subscription on a new Tenant each answer 201 echoing the tenant
/// and create it, the resource then served there and absent from the default Tenant
/// Clause: 5.5.10, 6.3.14, 5.6.1, 5.6.7, 5.6.11, 5.8.1, 5.9.2, 5.11.2, 4.14
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and says the other operations on a
/// non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates Tenants implicitly, exactly on the six
/// operations the clause lists
/// Tenant: private (the subject is creating Tenants)
#[gtest]
#[tokio::test]
async fn o_ten_imp_0004_each_creating_operation_creates_the_tenant_it_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    broker::private_tenant(&broker, &namespace).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let notify = config.unreachable_origin().join("notify")?;
    let tenants = (1..=6).map(|n| format!("{namespace}-t{n}").parse::<Tenant>()).collect::<Result<Vec<_>, _>>()?;
    let [t1, t2, t3, t4, t5, t6] = tenants.as_slice() else {
        return fail!("six tenants are named");
    };
    let names =
        |head: &ResponseHead, tenant: &Tenant| -> Result<()> { verify_that!(head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant), some(eq(tenant))) };

    // Create Entity
    let vehicle_1_id = namespace.urn("Vehicle", 1)?;
    let vehicle_1 = Entity::builder()
        .id(vehicle_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "v1")
        .build()?;
    let in_t1 = broker.with_tenant(t1.clone());
    let created = in_t1.with_heads().entities().create(&vehicle_1).send().await?;
    names(&created.head, t1)?;
    let retrieved = in_t1
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_1_id)
        .send()
        .await?;
    names(&retrieved.head, t1)?;
    expect_that!(&retrieved.outcome.body().id, eq(&vehicle_1_id));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_1_id)
        .send()
        .await;
    expect_that!(
        &absent,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );

    // Batch Entity Creation
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_2 = [Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "v2")
        .build()?];
    let in_t2 = broker.with_tenant(t2.clone());
    let created_2 = in_t2.with_heads().entities().batch_create(&vehicle_2).send().await?;
    names(&created_2.head, t2)?;
    created_2.outcome.done()?;
    let retrieved_2 = in_t2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    names(&retrieved_2.head, t2)?;
    expect_that!(&retrieved_2.outcome.body().id, eq(&vehicle_2_id));
    let absent_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await;
    expect_that!(
        &absent_2,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );

    // Create or Update Temporal Evolution
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_3 = TemporalEntity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(55).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    let in_t3 = broker.with_tenant(t3.clone());
    let created_3 = in_t3.with_heads().temporal().create_or_update(&vehicle_3).send().await?;
    names(&created_3.head, t3)?;
    created_3.outcome.created()?;
    let retrieved_3 = in_t3
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_3_id)
        .send()
        .await?;
    names(&retrieved_3.head, t3)?;
    expect_that!(&retrieved_3.outcome.body().id, eq(&vehicle_3_id));
    let absent_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_3_id)
        .send()
        .await;
    expect_that!(
        &absent_3,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );

    // Create Subscription
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(notify.clone()).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let in_t4 = broker.with_tenant(t4.clone());
    let created_4 = in_t4.with_heads().subscriptions().create(&subscription).send().await?;
    names(&created_4.head, t4)?;
    let retrieved_4 = in_t4
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    names(&retrieved_4.head, t4)?;
    expect_that!(retrieved_4.outcome.id(), eq(&subscription_id));
    let absent_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(
        &absent_4,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );

    // Register Context Source
    let registration_id = namespace.registration_id(1)?;
    let vehicle_a456_id = namespace.urn("Vehicle", "A456")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_a456_id)])
            .property_names(["brandName", "speed"])
            .relationship_names(["isParked"])
            .build()?])
        .build()?;
    let in_t5 = broker.with_tenant(t5.clone());
    let created_5 = in_t5.with_heads().registrations().register(&registration).send().await?;
    names(&created_5.head, t5)?;
    let retrieved_5 = in_t5
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    names(&retrieved_5.head, t5)?;
    expect_that!(retrieved_5.outcome.id(), eq(&registration_id));
    let absent_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(
        &absent_5,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );

    // Create Context Source Registration Subscription
    let registration_subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(notify).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let in_t6 = broker.with_tenant(t6.clone());
    let created_6 = in_t6
        .with_heads()
        .registration_subscriptions()
        .create(&registration_subscription)
        .send()
        .await?;
    names(&created_6.head, t6)?;
    let retrieved_6 = in_t6
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    names(&retrieved_6.head, t6)?;
    expect_that!(retrieved_6.outcome.id(), eq(&subscription_id));
    let absent_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(
        &absent_6,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).without_header(HeaderName::from_static("ngsild-tenant"))
    );
    Ok(())
}
