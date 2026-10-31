//! An unquoted uri containing dots is a valid q value on a property and a relationship.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType, Uri},
    query::q,
};

/// Pins: a q whose operand is an unquoted URI containing dots (an http host, a dotted path, a port, a dotted URN)
/// is well-formed, and == and != select by the value of a String Property and by the object of a Relationship
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3010_an_unquoted_uri_containing_dots_is_a_valid_q_value_on_a_property_and_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let thing_2_id = namespace.urn("Thing", 2)?;
    let thing_3_id = namespace.urn("Thing", 3)?;
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
            .relationship("ref", "https://sub.example.org/dir.v2/target.json".parse::<EntityId>()?)
            .build()?,
        Entity::builder()
            .id(thing_3_id.clone())
            .entity_type(thing_type.clone())
            .property("link", "http://example.org:8080/a")
            .relationship("ref", "urn:example:release:1.5".parse::<EntityId>()?)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // CompEqualityValue = OtherValue / ValueList / Range / URI: "URI shall be a URI as mandated by IETF RFC 3986", and an http URI carries dots
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_id)));
    // several dots in the host and the path
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("https://sub.example.org/a.b.c".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&thing_2_id)));
    // a port in the authority
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org:8080/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&thing_3_id)));
    // the object of a Relationship
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("http://example.org/target".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&thing_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("https://sub.example.org/dir.v2/target.json".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&thing_2_id)));
    // a URN with a dot
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("ref").eq(q::UnquotedUri::try_from("urn:example:release:1.5".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&thing_3_id)));
    // != with a dotted URI selects the entities whose value or object is another
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("link").ne(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(2)));
    expect_that!(page_7.items(), contains(has_id(&thing_2_id)));
    expect_that!(page_7.items(), contains(has_id(&thing_3_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("ref").ne(q::UnquotedUri::try_from("http://example.org/target".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(2)));
    expect_that!(page_8.items(), contains(has_id(&thing_2_id)));
    expect_that!(page_8.items(), contains(has_id(&thing_3_id)));
    // the dotted URI beside another term of the combination
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&thing_type)
        .q(q::attr("link")
            .eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?)
            .and(q::attr("ref").eq(q::UnquotedUri::try_from("http://example.org/target".parse::<Uri>()?)?)))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), nth(0, has_id(&thing_id)));
    Ok(())
}
