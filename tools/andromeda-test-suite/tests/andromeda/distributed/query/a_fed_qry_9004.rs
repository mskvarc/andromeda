//! A cursor page counts the whole union.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    followed_link::request_with,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    paged_federation::{local_entities, serve_pages},
    problem::answered,
    unchecked_outcome::read_as,
    wire::header_value,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::QueryEntities,
    },
    model::{Lossless, NgsiLdMediaType},
    query::{IdPattern, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities over an inclusive registration whose source holds 2,500 Entities in pages of 1,000, beside
/// 14 the broker holds, answers at the next pointer of a first page of 100 the second 100 identifiers in identifier
/// order, counting the whole 2,504
/// Clause: 5.7.2.4, 5.5.9.1, 6.3.13
#[gtest]
#[tokio::test]
async fn a_fed_qry_9004_a_cursor_page_counts_the_whole_union() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_vehicle_type = namespace.entity_type("PagedVehicle")?;
    let registration_id = namespace.registration_id(1)?;
    serve_pages(&source, &namespace).await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(paged_vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker.entities().batch_create(&local_entities(&namespace)?).send().await?.done()?;
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&paged_vehicle_type)
        .limit(100)
        .send()
        .await?;
    expect_that!(first.outcome.items(), len(eq(100)));
    expect_that!(first.outcome.items(), nth(0, has_id(&namespace.urn("PagedVehicle", "00000")?)));
    expect_that!(first.outcome.items(), nth(50, has_id(&namespace.urn("PagedVehicle", "00049")?)));
    expect_that!(first.outcome.items(), nth(99, has_id(&namespace.urn("PagedVehicle", "00098")?)));
    let next_link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    let selector = SelectorTypeBuilder::from(&paged_vehicle_type).build()?;
    let query = QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build();
    let next_request =
        request_with(&broker, &next_link, &RelationType::Next, &[("count", "true")])?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let next_sent = next_request.as_request().clone();
    let next_answer = broker.send_unchecked(next_request).await;
    expect_that!(&next_answer, answered(StatusCode::OK));
    let next_response = next_answer?;
    expect_that!(
        header_value(next_response.headers(), &HeaderName::from_static("ngsild-results-count")),
        some(eq("2504"))
    );
    let next_page = read_as(&query, &next_sent, next_response)?;
    expect_that!(next_page.items(), len(eq(100)));
    expect_that!(next_page.items(), nth(0, has_id(&namespace.urn("PagedVehicle", "00099")?)));
    expect_that!(next_page.items(), nth(50, has_id(&namespace.urn("PagedVehicle", "00149")?)));
    expect_that!(next_page.items(), nth(99, has_id(&namespace.urn("PagedVehicle", "00198")?)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
