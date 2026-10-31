//! A uri rfc 3986 does not make equivalent does not match.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with q `isParkedIn` equal to `http://example.com/A/C`, `http://example.com/a/d`,
/// `http://example.org/a/c` or `https://example.com/a/c` finds nothing when the `isParkedIn` object is
/// `http://example.com/a/c`
/// Clause: 4.9, 5.5.1, 5.7.2; IETF RFC 3986 sections 6.2.2.1 and 6.2.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9048_a_uri_rfc_3986_does_not_make_equivalent_does_not_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let uri_comparison_distinct_id = namespace.urn("UriComparison", "distinct")?;
    let uri_comparison_type = namespace.entity_type("UriComparison")?;
    let uri_comparison_distinct = Entity::builder()
        .id(uri_comparison_distinct_id.clone())
        .entity_type(uri_comparison_type.clone())
        .relationship("isParkedIn", "http://example.com/a/c".parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&uri_comparison_distinct).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_distinct_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.com/A/C"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_distinct_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.com/a/d"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_distinct_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.org/a/c"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_distinct_id.clone()])
        .q(q::attr("isParkedIn").eq("https://example.com/a/c"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
