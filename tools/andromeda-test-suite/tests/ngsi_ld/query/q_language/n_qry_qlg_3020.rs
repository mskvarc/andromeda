//! A trailing path into a json property named in json keys supports every operator.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, JsonProperty, NgsiLdMediaType, RawJson},
    query::q::Query,
};

/// Pins: a q whose trailing path in square brackets names a member of a `JsonProperty` listed in `jsonKeys` is
/// evaluated against that member, raw and unexpanded, for each operator of the grammar and for the bare existence
/// of the path, although the member is a term of the default context
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3020_a_trailing_path_into_a_json_property_named_in_json_keys_supports_every_operator() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let tickets = format!("tickets_{namespace}");
    let car_id = |n: usize| namespace.urn("Car", n);
    let bodies = [
        format!(r#"{{"id": "{namespace}-85a6cc52", "value": "Overstay 60 minutes", "object": 10}}"#),
        format!(r#"{{"id": "{namespace}-17b4d0e1", "value": "Paid", "object": 30}}"#),
        format!(r#"{{"id": "{namespace}-no-value-here"}}"#),
    ];
    let mut entities = Vec::new();
    for (n, body) in bodies.iter().enumerate() {
        entities.push(
            Entity::builder()
                .id(car_id(n + 1)?)
                .entity_type(car_type.clone())
                .json_property(tickets.as_str(), JsonProperty::builder(body.parse::<RawJson>()?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Clause 4.9 EXAMPLE 11: the target value raw is JSON, and is not expanded.
    let cases: [(String, &[usize], bool, &[usize]); 11] = [
        (format!(r#"{tickets}[value]=="Overstay 60 minutes""#), &[1], true, &[]),
        (format!(r#"{tickets}[value]!="Overstay 60 minutes""#), &[2], false, &[1]),
        (format!(r#"{tickets}[id]=="{namespace}-17b4d0e1""#), &[2], true, &[]),
        (format!("{tickets}[object]>20"), &[2], true, &[]),
        (format!("{tickets}[object]<=10"), &[1], true, &[]),
        (format!("{tickets}[object]>=10"), &[1, 2], true, &[]),
        (format!("{tickets}[object]<30"), &[1], true, &[]),
        (format!("{tickets}[object]==5..15"), &[1], true, &[]),
        (format!("{tickets}[object]==10,30"), &[1, 2], true, &[]),
        (format!(r#"{tickets}[value]~="^Overstay""#), &[1], true, &[]),
        (format!("{tickets}[value]"), &[1, 2], true, &[]),
    ];
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for (q, expected, exact, forbidden) in &cases {
        let page = json
            .entities()
            .query()
            .entity_type(&car_type)
            .q(q.parse::<Query>()?)
            .json_keys(vec![tickets.parse::<AttributeName>()?])
            .send()
            .await?;
        if *exact {
            expect_that!(page.items(), len(eq(expected.len())));
        }
        for n in *expected {
            expect_that!(page.items(), contains(has_id(&car_id(*n)?)));
        }
        for n in *forbidden {
            expect_that!(page.items(), not(contains(has_id(&car_id(*n)?))));
        }
    }
    Ok(())
}
