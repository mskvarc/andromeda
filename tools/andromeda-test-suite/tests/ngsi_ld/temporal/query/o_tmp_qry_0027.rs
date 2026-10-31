//! A counted cursor page without local counts the whole collection.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
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
    },
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities without local followed by its next pointer with count true added, over
/// five entities, answers the third and fourth entities and NGSILD-Results-Count 5
/// Clause: 6.3.13, 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute to it;
/// in the shared tenant a registration of another file naming no entity type matches every type (clause 5.12) and makes
/// the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a query
/// in its default order by an opaque cursor and offers first and last pointers beside next and prev
/// Recorded: no order is fixed for a query without orderBy; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0027_a_counted_cursor_page_without_local_counts_the_whole_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0003_id = namespace.urn("Sensor", "0003")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    tenant.registrations().delete(&registration_provision_id).send().await?;
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
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(headed.outcome.next(), some(anything()));
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
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
