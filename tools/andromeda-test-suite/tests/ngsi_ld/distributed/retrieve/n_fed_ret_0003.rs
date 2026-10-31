//! A retrieve is forwarded as a retrieve to every group holding retrieve entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Relationship},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo},
};

/// Pins: Retrieve Entity is forwarded exactly once, as GET /entities/{id} and never as a query, to a registration
/// advertising `retrieveOps`, `redirectionOps`, `federationOps`, `associationOps` or no operations member, and
/// answers the Entity the source returns
/// Clause: 5.7.1.4, 4.20, 5.2.9
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_ret_0003_a_retrieve_is_forwarded_as_a_retrieve_to_every_group_holding_retrieve_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let registration_5_id = namespace.registration_id(5)?;
    let vehicle_5_id = namespace.urn("Vehicle", 5)?;
    let registration = CSourceRegistration::builder(source.endpoint("retrieveOps")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([OperationGroup::RetrieveOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/retrieveOps/ngsi-ld/v1/entities/[^/]+"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("redirectionOps")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let source_entity_2 = Entity::output_builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/redirectionOps/ngsi-ld/v1/entities/[^/]+"),
            source_answer::entity(&source_entity_2),
        )
        .await?;
    let registration_3 = CSourceRegistration::builder(source.endpoint("federationOps")?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .build()?])
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let source_entity_3 = Entity::output_builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/federationOps/ngsi-ld/v1/entities/[^/]+"),
            source_answer::entity(&source_entity_3),
        )
        .await?;
    let registration_4 = CSourceRegistration::builder(source.endpoint("associationOps")?)
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_4_id.clone())])
            .build()?])
        .operations([OperationGroup::AssociationOps])
        .build()?;
    broker.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&broker).registration(&registration_4_id);
    let source_entity_4 = Entity::output_builder()
        .id(vehicle_4_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/associationOps/ngsi-ld/v1/entities/[^/]+"),
            source_answer::entity(&source_entity_4),
        )
        .await?;
    let registration_5 = CSourceRegistration::builder(source.endpoint("none")?)
        .id(registration_5_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_5_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_5).send().await?;
    let _registration_5_cleanup = Cleanup::of(&broker).registration(&registration_5_id);
    let source_entity_5 = Entity::output_builder()
        .id(vehicle_5_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/none/ngsi-ld/v1/entities/[^/]+"),
            source_answer::entity(&source_entity_5),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_request = SourceRequest::matching(Method::GET, "/retrieveOps/ngsi-ld/v1/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::any_matching("/retrieveOps/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_2_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&vehicle_2_id));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_request_3 = SourceRequest::matching(Method::GET, "/redirectionOps/ngsi-ld/v1/entities/[^/]+");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    let forwarded_request_4 = SourceRequest::any_matching("/redirectionOps/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(1)));
    let retrieved_3 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_3_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&vehicle_3_id));
    expect_that!(entity_3, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_request_5 = SourceRequest::matching(Method::GET, "/federationOps/ngsi-ld/v1/entities/[^/]+");
    let forwarded_5 = source.received_matching(&forwarded_request_5).await?;
    expect_that!(forwarded_5, len(eq(1)));
    let forwarded_request_6 = SourceRequest::any_matching("/federationOps/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_6 = source.received_matching(&forwarded_request_6).await?;
    expect_that!(forwarded_6, len(eq(1)));
    let retrieved_4 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_4_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.id, eq(&vehicle_4_id));
    expect_that!(entity_4, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_request_7 = SourceRequest::matching(Method::GET, "/associationOps/ngsi-ld/v1/entities/[^/]+");
    let forwarded_7 = source.received_matching(&forwarded_request_7).await?;
    expect_that!(forwarded_7, len(eq(1)));
    let forwarded_request_8 = SourceRequest::any_matching("/associationOps/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_8 = source.received_matching(&forwarded_request_8).await?;
    expect_that!(forwarded_8, len(eq(1)));
    let retrieved_5 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_5_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(entity_5.id, eq(&vehicle_5_id));
    expect_that!(entity_5, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_request_9 = SourceRequest::matching(Method::GET, "/none/ngsi-ld/v1/entities/[^/]+");
    let forwarded_9 = source.received_matching(&forwarded_request_9).await?;
    expect_that!(forwarded_9, len(eq(1)));
    let forwarded_request_10 = SourceRequest::any_matching("/none/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_10 = source.received_matching(&forwarded_request_10).await?;
    expect_that!(forwarded_10, len(eq(1)));
    Ok(())
}
