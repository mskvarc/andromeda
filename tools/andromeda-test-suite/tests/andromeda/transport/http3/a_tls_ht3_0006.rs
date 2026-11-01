//! An entity created over HTTP/3 is retrieved over TCP.

use andromeda_test_suite::{
    attribute_matcher::property,
    config::BrokerRole,
    entity_matcher::has_attribute,
    namespace::Namespace,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use http::{StatusCode, Version};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: an Entity created by a client speaking only HTTP/3 is answered by Retrieve Entity over the TCP listener of
/// the same port with the Property it was created with: both transports serve one broker
/// Clause: 5.6.1, 5.7.1, 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0006_an_entity_created_over_http3_is_retrieved_over_tcp() -> Result<()> {
    let namespace = Namespace::fresh();
    let quic = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 42)
        .build()?;
    let created = quic.with_heads().entities().create(&vehicle).send().await?;
    expect_that!(created.status, eq(StatusCode::CREATED));
    expect_that!(created.head.version(), eq(Version::HTTP_3));
    let tcp = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let retrieved = tcp
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved.status, eq(StatusCode::OK));
    expect_that!(retrieved.head.version(), eq(Version::HTTP_2));
    let entity = retrieved.outcome.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, has_attribute("speed", property().value(42)));
    Ok(())
}
