//! An unquoted uri containing dots is a valid q value.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{EntityId, NgsiLdMediaType, Property, Relationship, Uri, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities accepts a q whose operand is an unquoted URI containing dots, through
/// the GET parameter and the POST member, and selects the entity whose Property value or Relationship object equals
/// it
/// Clause: 4.9, 5.7.4.4, 6.18.3.2, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3060_an_unquoted_uri_containing_dots_is_a_valid_q_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let thing_2_id = namespace.urn("Thing", 2)?;
    let thing_evolution = TemporalEntity::builder()
        .id(thing_id.clone())
        .entity_type(thing_type.clone())
        .property("link", Property::builder("http://example.org/a").observed_at("2020-01-01T00:00:00Z".parse()?))
        .relationship(
            "ref",
            Relationship::builder("http://example.org/target".parse::<EntityId>()?).observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&thing_evolution).send().await?.created()?;
    let thing_2_evolution = TemporalEntity::builder()
        .id(thing_2_id.clone())
        .entity_type(thing_type.clone())
        .property(
            "link",
            Property::builder("https://sub.example.org/a.b.c").observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .relationship(
            "ref",
            Relationship::builder("urn:example:release:1.5".parse::<EntityId>()?).observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&thing_2_evolution).send().await?.created()?;
    // CompEqualityValue = OtherValue / ValueList / Range / URI; the URI carries dots
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("urn:example:release:1.5".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&thing_2_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("http://example.org/target".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&thing_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("https://sub.example.org/a.b.c".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&thing_2_id)));
    Ok(())
}
