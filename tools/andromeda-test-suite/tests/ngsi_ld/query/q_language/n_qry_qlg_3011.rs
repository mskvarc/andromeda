//! Post query an unquoted uri containing dots is a valid q value.

use andromeda_test_suite::{broker, core_context::core_context, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Entity, EntityId, NgsiLdMediaType, Uri},
    query::q,
};

/// Pins: Query Entities by POST accepts a q member whose operand is an unquoted URI containing dots and selects by
/// the Property value and the Relationship object it equals, under an application/json and an application/ld+json
/// body
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3011_post_query_an_unquoted_uri_containing_dots_is_a_valid_q_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let thing_2_id = namespace.urn("Thing", 2)?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let entities = [
        Entity::builder()
            .id(thing_id.clone())
            .entity_type(thing_type.clone())
            .property("link", "http://example.org/a")
            .relationship("ref", "http://example.org/target".parse::<EntityId>()?)
            .build()?,
        Entity::builder()
            .id(thing_2_id.clone())
            .entity_type(thing_type.clone())
            .property("link", "https://sub.example.org/a.b.c")
            .relationship("ref", "urn:example:release:1.5".parse::<EntityId>()?)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // CompEqualityValue = OtherValue / ValueList / Range / URI; the URI carries dots
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("urn:example:release:1.5".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&thing_2_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("link").ne(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&thing_2_id)));
    let page_4 = core_context_body
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("http://example.org/target".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&thing_id)));
    Ok(())
}
