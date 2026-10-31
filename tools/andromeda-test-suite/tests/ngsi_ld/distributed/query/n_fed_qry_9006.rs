//! A failing source page keeps the gathered entities with a warning.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Output},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities at limit 1,000 and count true over an inclusive registration whose source holds 2,500
/// Entities answered in pages of 1,000 linked by next, beside 14 the broker holds itself (4 interleaving the
/// source's identifiers, 10 shared with it), whose second page fails with 500, answers 200 with the 1,000 Entities
/// of its first page and the broker's 14, counting 1,014, and an NGSILD-Warning
/// Clause: 5.7.2.4, 6.3.17, 6.3.13
#[gtest]
#[tokio::test]
async fn n_fed_qry_9006_a_failing_source_page_keeps_the_gathered_entities_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_vehicle_type = namespace.entity_type("PagedVehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let paged_vehicle_00000_l_id = namespace.urn("PagedVehicle", "00000L")?;
    let paged_vehicle_00999_l_id = namespace.urn("PagedVehicle", "00999L")?;
    let paged_vehicle_01000_l_id = namespace.urn("PagedVehicle", "01000L")?;
    let paged_vehicle_01500_l_id = namespace.urn("PagedVehicle", "01500L")?;
    let paged_vehicle_02490_id = namespace.urn("PagedVehicle", "02490")?;
    let paged_vehicle_02491_id = namespace.urn("PagedVehicle", "02491")?;
    let paged_vehicle_02492_id = namespace.urn("PagedVehicle", "02492")?;
    let paged_vehicle_02493_id = namespace.urn("PagedVehicle", "02493")?;
    let paged_vehicle_02494_id = namespace.urn("PagedVehicle", "02494")?;
    let paged_vehicle_02495_id = namespace.urn("PagedVehicle", "02495")?;
    let paged_vehicle_02496_id = namespace.urn("PagedVehicle", "02496")?;
    let paged_vehicle_02497_id = namespace.urn("PagedVehicle", "02497")?;
    let paged_vehicle_02498_id = namespace.urn("PagedVehicle", "02498")?;
    let paged_vehicle_02499_id = namespace.urn("PagedVehicle", "02499")?;
    // The source holds 2,500 Entities and answers them in pages of 1,000 linked by next; the mock server generates each
    // page, numbering the Entities from 00000.
    let source_entities = (0..=999)
        .map(|n: i64| -> Result<Entity<Output>> {
            let numbered_id = namespace.urn("PagedVehicle", format!("{n:05}"))?;
            Ok(Entity::output_builder()
                .id(numbered_id)
                .entity_type(paged_vehicle_type.clone())
                .property("remote", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").priority(5),
            source_answer::linked_to_next(
                source_answer::entities(&source_entities),
                format!("/a/ngsi-ld/v1/entities?type=PagedVehicle_{namespace}&limit=1000&offset=1000").parse()?,
            )?,
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("offset", "1000").priority(1),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(paged_vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let entities = vec![
        Entity::builder()
            .id(paged_vehicle_00000_l_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_00999_l_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_01000_l_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_01500_l_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02490_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02491_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02492_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02493_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02494_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02495_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02496_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02497_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02498_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02499_id)
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&paged_vehicle_type)
        .limit(1000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.warnings(), present());
    expect_that!(page.total(), some(eq(1014)));
    expect_that!(page.items(), len(eq(1000)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
