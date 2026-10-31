//! An entity held on both sides is merged once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Output},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities at offset 2,490 over an inclusive registration whose source holds 2,500 Entities answered
/// in pages of 1,000 linked by next, beside 14 the broker holds itself (4 interleaving the source's identifiers, 10
/// shared with it) answers each of the 10 shared Entities once, with the broker's and the source's attribute, and
/// counts 2,504
/// Clause: 5.7.2.4, 4.5.5
#[gtest]
#[tokio::test]
async fn a_fed_qry_9005_an_entity_held_on_both_sides_is_merged_once() -> Result<()> {
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
    let source_entities_2 = (1000..=1999)
        .map(|n: i64| -> Result<Entity<Output>> {
            let numbered_id_2 = namespace.urn("PagedVehicle", format!("{n:05}"))?;
            Ok(Entity::output_builder()
                .id(numbered_id_2)
                .entity_type(paged_vehicle_type.clone())
                .property("remote", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("offset", "1000").priority(1),
            source_answer::linked_to_next(
                source_answer::entities(&source_entities_2),
                format!("/a/ngsi-ld/v1/entities?type=PagedVehicle_{namespace}&limit=1000&offset=2000").parse()?,
            )?,
        )
        .await?;
    let source_entities_3 = (2000..=2499)
        .map(|n: i64| -> Result<Entity<Output>> {
            let numbered_id_3 = namespace.urn("PagedVehicle", format!("{n:05}"))?;
            Ok(Entity::output_builder()
                .id(numbered_id_3)
                .entity_type(paged_vehicle_type.clone())
                .property("remote", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("offset", "2000").priority(1),
            source_answer::entities(&source_entities_3),
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
            .id(paged_vehicle_02490_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02491_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02492_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02493_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02494_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02495_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02496_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02497_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02498_id.clone())
            .entity_type(paged_vehicle_type.clone())
            .property("local", true)
            .build()?,
        Entity::builder()
            .id(paged_vehicle_02499_id.clone())
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
        .limit(20)
        .offset(2490)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2504)));
    expect_that!(page.items(), len(eq(14)));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02490_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2490))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02491_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2491))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02492_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2492))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02493_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2493))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02494_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2494))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02495_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2495))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02496_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2496))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02497_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2497))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02498_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2498))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&paged_vehicle_02499_id),
            has_attribute("local", any_attribute().value(true)),
            has_attribute("remote", any_attribute().value(2499))
        ))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
