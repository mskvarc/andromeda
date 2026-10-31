//! Every rfc 3986 uri shape is accepted as a relationship object.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
};

/// Pins: Create Entity with Relationships whose objects are a URN, a UUID URN, a tag, mailto, did, file, ftp, s3,
/// coap+tcp, a custom scheme, an ldap URI with an `IPv6` host and an https URI with query and fragment answers 201,
/// and Retrieve Entity reads each object back
/// Clause: 4.5.3.2, 5.2.6, 5.6.1, 5.7.1; IETF RFC 3986 section 3 and appendix A
#[gtest]
#[tokio::test]
async fn n_ent_cre_9031_every_rfc_3986_uri_shape_is_accepted_as_a_relationship_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let uri_comparison_shapes_id = namespace.urn("UriComparison", "shapes")?;
    let uri_comparison_type = namespace.entity_type("UriComparison")?;
    let building_001_id = namespace.urn("Building", "001")?;
    let uuid_6e8bc430_9c3a_id = "urn:uuid:6e8bc430-9c3a-11d9-9669-0800200c9a66".parse::<EntityId>()?;
    let example_com_2024_id = "tag:example.com,2024:foo".parse::<EntityId>()?;
    let someone_example_com_id = "mailto:someone@example.com".parse::<EntityId>()?;
    let example_123_id = "did:example:123".parse::<EntityId>()?;
    let tmp_x_id = "file:///tmp/x".parse::<EntityId>()?;
    let example_com_x_id = "ftp://example.com/x".parse::<EntityId>()?;
    let bucket_key_id = "s3://bucket/key".parse::<EntityId>()?;
    let host_x_id = "coap+tcp://host/x".parse::<EntityId>()?;
    let opaque_id = "x-custom.scheme+v1:opaque".parse::<EntityId>()?;
    let id_2001_db8_7_id = "ldap://[2001:db8::7]/c=GB?objectClass?one".parse::<EntityId>()?;
    let example_com_a_id = "https://example.com/a?b=c#d".parse::<EntityId>()?;
    let uri_comparison_shapes = Entity::builder()
        .id(uri_comparison_shapes_id.clone())
        .entity_type(uri_comparison_type)
        .relationship("shape0", building_001_id.clone())
        .relationship("shape1", "urn:uuid:6e8bc430-9c3a-11d9-9669-0800200c9a66".parse::<EntityId>()?)
        .relationship("shape2", "tag:example.com,2024:foo".parse::<EntityId>()?)
        .relationship("shape3", "mailto:someone@example.com".parse::<EntityId>()?)
        .relationship("shape4", "did:example:123".parse::<EntityId>()?)
        .relationship("shape5", "file:///tmp/x".parse::<EntityId>()?)
        .relationship("shape6", "ftp://example.com/x".parse::<EntityId>()?)
        .relationship("shape7", "s3://bucket/key".parse::<EntityId>()?)
        .relationship("shape8", "coap+tcp://host/x".parse::<EntityId>()?)
        .relationship("shape9", "x-custom.scheme+v1:opaque".parse::<EntityId>()?)
        .relationship("shape10", "ldap://[2001:db8::7]/c=GB?objectClass?one".parse::<EntityId>()?)
        .relationship("shape11", "https://example.com/a?b=c#d".parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&uri_comparison_shapes).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&uri_comparison_shapes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("shape0", any_attribute().object(building_001_id.clone())));
    expect_that!(entity, has_attribute("shape1", any_attribute().object(uuid_6e8bc430_9c3a_id)));
    expect_that!(entity, has_attribute("shape2", any_attribute().object(example_com_2024_id)));
    expect_that!(entity, has_attribute("shape3", any_attribute().object(someone_example_com_id)));
    expect_that!(entity, has_attribute("shape4", any_attribute().object(example_123_id)));
    expect_that!(entity, has_attribute("shape5", any_attribute().object(tmp_x_id)));
    expect_that!(entity, has_attribute("shape6", any_attribute().object(example_com_x_id)));
    expect_that!(entity, has_attribute("shape7", any_attribute().object(bucket_key_id)));
    expect_that!(entity, has_attribute("shape8", any_attribute().object(host_x_id)));
    expect_that!(entity, has_attribute("shape9", any_attribute().object(opaque_id)));
    expect_that!(entity, has_attribute("shape10", any_attribute().object(id_2001_db8_7_id)));
    expect_that!(entity, has_attribute("shape11", any_attribute().object(example_com_a_id)));
    Ok(())
}
