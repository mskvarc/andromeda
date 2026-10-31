//! Post query a trailing path into a json property named in the json keys member.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_call::exchange,
    unchecked_page,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{AttributeName, Entity, JsonProperty, NgsiLdMediaType, RawJson},
    query::q::Query,
};

/// Pins: Query Entities by POST evaluates a q trailing path into a `JsonProperty` named in the `jsonKeys` member
/// against the raw member, whatever the key is called, under an application/json and an application/ld+json body
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3022_post_query_a_trailing_path_into_a_json_property_named_in_the_json_keys_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let car_type = namespace.entity_type("Car")?;
    let tickets = format!("tickets_{namespace}");
    let ids = [namespace.urn("Car", 1)?, namespace.urn("Car", 2)?];
    let documents = [
        format!(r#"{{"id": "{namespace}-85a6cc52", "value": "Overstay 60 minutes"}}"#),
        format!(r#"{{"id": "{namespace}-17b4d0e1", "value": "Paid"}}"#),
    ];
    let mut entities = Vec::new();
    for (id, document) in ids.iter().zip(&documents) {
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(car_type.clone())
                .json_property(tickets.as_str(), JsonProperty::builder(document.parse::<RawJson>()?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let posted = async |q: String| -> Result<_> {
        Ok(json
            .entities()
            .query()
            .method(QueryMethod::Post)
            .entity_type(&car_type)
            .q(q.parse::<Query>()?)
            .json_keys(vec![tickets.parse::<AttributeName>()?])
            .send()
            .await?)
    };
    let first = posted(format!(r#"{tickets}[value]=="Overstay 60 minutes""#)).await?;
    expect_that!(first.items(), len(eq(1)));
    expect_that!(first.items(), contains(has_id(&ids[0])));
    let second = posted(format!(r#"{tickets}[id]=="{namespace}-17b4d0e1""#)).await?;
    expect_that!(second.items(), len(eq(1)));
    expect_that!(second.items(), contains(has_id(&ids[1])));
    // The same query under an `application/ld+json` body carrying the context; the client writes the context in a Link field.
    let linked = format!(
        r#"{{"type": "Query", "entities": [{{"type": "{car_type}"}}], "q": "{tickets}[value]==\"Paid\"", "jsonKeys": "{tickets}", "@context": "{}"}}"#,
        config.core_context()
    );
    let answer = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "entityOperations/query",
            &[("Content-Type", "application/ld+json"), ("Accept", "application/json")],
            Some(linked.as_str()),
        )?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let linked_page = unchecked_page::entities(&answer?)?;
    expect_that!(linked_page, len(eq(1)));
    expect_that!(linked_page, contains(has_id(&ids[1])));
    let third = posted(format!(r#"{tickets}[value]=="Overstay 30 minutes""#)).await?;
    expect_that!(third.items(), is_empty());
    Ok(())
}
