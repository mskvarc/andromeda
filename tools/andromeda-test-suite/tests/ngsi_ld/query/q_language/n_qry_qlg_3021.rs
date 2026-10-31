//! A trailing path into json properties reaches nested members and every listed attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, JsonProperty, NgsiLdMediaType, RawJson},
    query::q::Query,
};
use std::result::Result as StdResult;

/// Pins: a trailing path into a `JsonProperty` named in `jsonKeys` reaches a nested member through a dotted path, a
/// Boolean member and a member of each of several attributes listed in `jsonKeys`, and addresses the raw key where
/// the key is also a term of the default context
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3021_a_trailing_path_into_json_properties_reaches_nested_members_and_every_listed_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let tickets = format!("tickets_{namespace}");
    let car_id = |n: usize| namespace.urn("Car", n);
    let permit = format!("permit_{namespace}");
    let documents = [
        (
            format!(r#"{{"owner": {{"name": "Ana", "type": "person_{namespace}"}}, "paid": true}}"#),
            r#"{"value": "gold", "level": 3}"#.to_owned(),
        ),
        (
            format!(r#"{{"owner": {{"name": "Ben", "type": "company_{namespace}"}}, "paid": false}}"#),
            r#"{"value": "silver", "level": 1}"#.to_owned(),
        ),
    ];
    let mut entities = Vec::new();
    for (n, (ticket, permit_json)) in documents.iter().enumerate() {
        entities.push(
            Entity::builder()
                .id(car_id(n + 1)?)
                .entity_type(car_type.clone())
                .json_property(tickets.as_str(), JsonProperty::builder(ticket.parse::<RawJson>()?))
                .json_property(permit.as_str(), JsonProperty::builder(permit_json.parse::<RawJson>()?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let cases: [(String, Vec<String>, &[usize]); 6] = [
        (format!(r#"{tickets}[owner.name]=="Ana""#), vec![tickets.clone()], &[1]),
        (format!(r#"{tickets}[owner.type]=="company_{namespace}""#), vec![tickets.clone()], &[2]),
        (format!("{tickets}[paid]==true"), vec![tickets.clone()], &[1]),
        (
            format!(r#"{tickets}[owner.name]=="Ben";{permit}[value]=="silver""#),
            vec![tickets.clone(), permit.clone()],
            &[2],
        ),
        (
            format!(r#"{tickets}[owner.name]=="Ana";{permit}[value]=="silver""#),
            vec![tickets.clone(), permit.clone()],
            &[],
        ),
        (
            format!("{permit}[level]>2|{tickets}[paid]==false"),
            vec![tickets.clone(), permit.clone()],
            &[1, 2],
        ),
    ];
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for (q, keys, expected) in &cases {
        let page = json
            .entities()
            .query()
            .entity_type(&car_type)
            .q(q.parse::<Query>()?)
            .json_keys(keys.iter().map(|key| key.parse::<AttributeName>()).collect::<StdResult<Vec<_>, _>>()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(expected.len())));
        for n in *expected {
            expect_that!(page.items(), contains(has_id(&car_id(*n)?)));
        }
    }
    Ok(())
}
