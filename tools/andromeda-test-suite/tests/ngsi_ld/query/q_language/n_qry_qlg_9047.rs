//! Every rfc 3986 equivalent spelling of a relationship object matches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with q `isParkedIn` equal to an upper-case scheme, an upper-case host, the default port 80,
/// a "." segment or a ".." segment spelling of `http://example.com/a/c` each finds the Entity whose `isParkedIn`
/// object is `http://example.com/a/c`
/// Clause: 4.9, 5.5.1, 5.7.2; IETF RFC 3986 sections 6.2.2.1, 6.2.2.3 and 6.2.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9047_every_rfc_3986_equivalent_spelling_of_a_relationship_object_matches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let uri_comparison_equivalences_id = namespace.urn("UriComparison", "equivalences")?;
    let uri_comparison_type = namespace.entity_type("UriComparison")?;
    let uri_comparison_equivalences = Entity::builder()
        .id(uri_comparison_equivalences_id.clone())
        .entity_type(uri_comparison_type.clone())
        .relationship("isParkedIn", "http://example.com/a/c".parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&uri_comparison_equivalences).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_equivalences_id.clone()])
        .q(q::attr("isParkedIn").eq("HTTP://Example.com/a/c"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&uri_comparison_equivalences_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_equivalences_id.clone()])
        .q(q::attr("isParkedIn").eq("http://EXAMPLE.COM/a/c"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&uri_comparison_equivalences_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_equivalences_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.com:80/a/c"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&uri_comparison_equivalences_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_equivalences_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.com/a/./c"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&uri_comparison_equivalences_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_equivalences_id.clone()])
        .q(q::attr("isParkedIn").eq("http://example.com/a/b/../c"))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), contains(has_id(&uri_comparison_equivalences_id)));
    Ok(())
}
