//! A forwarded query over Linked Entity Attributes is not refused as too deep.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntitiesParameters},
    model::{Entity, EntityId, EntityTypeName, ErrorType, OneOrMany, Output, Relationship},
    query::{EntitySplitting, Join, q, q::Query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::QueryDocument,
};
use std::{num::NonZeroU32, str};

/// The Vehicle `id` a source answers with, parked in the commercial Building `building`, which the source nests under
/// the Relationship as `join=inline` asks (clause 4.5.23.2).
fn parked(id: &EntityId, vehicle_type: &EntityTypeName, building: &EntityId, building_type: &EntityTypeName) -> Result<Entity<Output>> {
    let nested = Entity::projected_output_builder()
        .id(building.clone())
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .build()?;
    let is_parked_in = Relationship::<Output>::output_builder(Relationship::builder(building.clone()))
        .entity(OneOrMany::one(nested))
        .build()?;
    Ok(Entity::output_builder()
        .id(id.clone())
        .entity_type(vehicle_type.clone())
        .output_attribute("isParkedIn", is_parked_in)
        .build()?)
}

/// Pins: Query Entities with `q` over the Linked Entity Attribute `isParkedIn{category}`, join flat, joinLevel 1 and
/// `splitEntities=false` over an inclusive source advertising only `queryEntity` and one advertising only
/// `queryBatch`, each refusing a Linked Entity `q` without `join` with 400 `BadRequestData`, reaches the first with
/// the `q`, join inline and joinLevel 1 in its query string and the second with them as members of the posted Query,
/// and answers each Vehicle and each Building once
/// Clause: 5.7.2.4, 4.5.23, 6.23.3.1, Table 5.2.23-1
/// Recorded: a source refuses a `q` over Linked Entity Attributes without a retrieval as deep as its hops, so the
/// forward carries the client's `joinLevel` with join inline, in which the source nests its Linked Entities under the
/// Relationships rather than appending them as matches, and this broker renders the client's join itself
#[gtest]
#[tokio::test]
async fn n_fed_qry_9031_a_forwarded_query_over_linked_entity_attributes_is_not_refused_as_too_deep() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let get_vehicle_id = namespace.urn("Vehicle", "get")?;
    let batch_vehicle_id = namespace.urn("Vehicle", "batch")?;
    let get_building_id = namespace.urn("Building", "get")?;
    let batch_building_id = namespace.urn("Building", "batch")?;
    let information = RegistrationInfo::builder()
        .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
        .build()?;
    let get_registration_id = namespace.registration_id("get")?;
    let get_registration = CSourceRegistration::builder(source.endpoint("g")?)
        .id(get_registration_id.clone())
        .information([information.clone()])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    let batch_registration_id = namespace.registration_id("batch")?;
    let batch_registration = CSourceRegistration::builder(source.endpoint("b")?)
        .id(batch_registration_id.clone())
        .information([information])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    broker.registrations().register(&get_registration).send().await?;
    broker.registrations().register(&batch_registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registrations_cleanup = Cleanup::of(&broker).registration(&get_registration_id).registration(&batch_registration_id);
    // Clause 5.7.2.4: a source following the specification refuses a `q` over Linked Entity Attributes without a
    // retrieval as a too deep query.
    let too_deep = || {
        source_answer::problem(
            StatusCode::BAD_REQUEST,
            &source_answer::problem_without_detail(ErrorType::BadRequestData, "too deep query", Some(400)),
        )
    };
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/g/ngsi-ld/v1/entities").query("join", "inline"),
            source_answer::entities(&[parked(&get_vehicle_id, &vehicle_type, &get_building_id, &building_type)?]),
        )
        .await?;
    source
        .answer_when(SourceRequest::new(Method::GET, "/g/ngsi-ld/v1/entities").without_query("join"), too_deep())
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/b/ngsi-ld/v1/entityOperations/query").body(BodyPattern::Selects("$.join".parse()?)),
            source_answer::entities(&[parked(&batch_vehicle_id, &vehicle_type, &batch_building_id, &building_type)?]),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/b/ngsi-ld/v1/entityOperations/query").body(BodyPattern::DoesNotContain("\"join\"".to_owned())),
            too_deep(),
        )
        .await?;
    let commercial: Query = q::attr("isParkedIn").linked("category").eq("commercial").build()?;
    let level = NonZeroU32::MIN;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(commercial.clone())
        .join(Join::Flat)
        .join_level(level)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?;
    // Clause 4.5.23.3: the flat answer holds each Vehicle the query matched and each Building it links to, once.
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), contains(has_id(&get_vehicle_id)));
    expect_that!(page.items(), contains(has_id(&batch_vehicle_id)));
    expect_that!(page.items(), contains(has_id(&get_building_id)));
    expect_that!(page.items(), contains(has_id(&batch_building_id)));
    let got = source.received_on(Method::GET, "/g/ngsi-ld/v1/entities").await?;
    let got = got.first().or_fail()?;
    let get_uri = QueryEntitiesParameters::parse(got.url.query().unwrap_or_default())?;
    expect_that!(get_uri.selection.q.as_ref(), some(eq(&commercial)));
    expect_that!(get_uri.join, some(eq(Join::Inline)));
    expect_that!(get_uri.join_level, some(eq(level)));
    expect_that!(get_uri.contained_by, is_empty());
    let posted = source.received_on(Method::POST, "/b/ngsi-ld/v1/entityOperations/query").await?;
    let posted = posted.first().or_fail()?;
    let batch_query: QueryDocument = QueryDocument::parse(str::from_utf8(&posted.body)?)?;
    expect_that!(batch_query.q(), some(eq(&commercial)));
    expect_that!(batch_query.join(), some(eq(Join::Inline)));
    expect_that!(batch_query.join_level(), some(eq(level)));
    Ok(())
}
