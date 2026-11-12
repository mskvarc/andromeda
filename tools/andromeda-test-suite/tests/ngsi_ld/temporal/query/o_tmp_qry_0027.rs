//! A counted cursor page without local counts the whole collection.

use andromeda_test_suite::{
    broker,
    cursor_walk,
    followed_link::request_with,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Upserted,
    temporal_page_body,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, Link, NgsiLdResultsCount, RelationType},
        operation::{NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities without local, started with an empty cursor and followed by its next
/// pointer with count true added, over five entities, answers the third and fourth entities and NGSILD-Results-Count 5
/// Clause: 6.3.13, 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute to it;
/// in the shared tenant a registration of another file naming no entity type matches every type (clause 5.12) and makes
/// the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a query
/// in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last pointers
/// beside next and prev
/// Recorded: no order is fixed for a query without orderBy; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0027_a_counted_cursor_page_without_local_counts_the_whole_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0003_id = namespace.urn("Sensor", "0003")?;
    let evolutions = (0..=4)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("Sensor", format!("{n:04}"))?)
                .entity_type(sensor_type.clone())
                .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        tenant.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let window = temporal_query::after("1970-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&sensor_type).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .limit(2)
        .format(NormalizedTemporal)
        .build();
    let walk = cursor_walk::first_page(&tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), &query).await?;
    expect_that!(walk.page.next(), some(anything()));
    let link = walk.head.typed::<Link>().into_valid().or_fail()?;
    let request =
        request_with(&tenant, &link, &RelationType::Next, &[("count", "true")])?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = tenant.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let entities = temporal_page_body::entities(&answer)?;
    expect_that!(entities.as_slice(), ids_in_order(&[sensor_0002_id, sensor_0003_id]));
    let counted = ResponseHead::new(answer.headers().clone())
        .typed::<NgsiLdResultsCount>()
        .valid()
        .map(|count| count.count());
    expect_that!(counted, some(eq(5)));
    Ok(())
}
