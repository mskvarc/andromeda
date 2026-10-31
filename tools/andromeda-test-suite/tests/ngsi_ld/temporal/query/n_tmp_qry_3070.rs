//! A trailing path into a json property named in json keys is evaluated raw.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{JsonProperty, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities evaluates a q trailing path into a `JsonProperty` named in `jsonKeys`
/// against the raw member, through the GET parameters and the POST members
/// Clause: 4.9, 5.7.4.4, 6.18.3.2, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3070_a_trailing_path_into_a_json_property_named_in_json_keys_is_evaluated_raw() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let car_1_id = namespace.urn("Car", 1)?;
    let car_2_id = namespace.urn("Car", 2)?;
    let tickets = namespace.suffixed("tickets");
    for (id, ticket) in [
        (&car_1_id, format!(r#"{{ "id": "{namespace}-85a6cc52", "value": "Overstay 60 minutes" }}"#)),
        (&car_2_id, format!(r#"{{ "id": "{namespace}-17b4d0e1", "value": "Paid" }}"#)),
    ] {
        let car = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(car_type.clone())
            .json_property(
                namespace.attribute("tickets")?,
                JsonProperty::builder(ticket.parse::<RawJson>()?).observed_at("2020-01-01T00:00:00Z".parse()?),
            )
            .build()?;
        broker.temporal().create_or_update(&car).send().await?.created()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let by_get = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr(&tickets).member("value").eq("Overstay 60 minutes"))
        .json_keys(vec![namespace.attribute("tickets")?])
        .send()
        .await?;
    expect_that!(by_get.items(), len(eq(1)));
    expect_that!(by_get.items(), nth(0, has_id(&car_1_id)));
    let by_get_2 = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr(&tickets).member("id").eq(format!("{namespace}-17b4d0e1")))
        .json_keys(vec![namespace.attribute("tickets")?])
        .send()
        .await?;
    expect_that!(by_get_2.items(), len(eq(1)));
    expect_that!(by_get_2.items(), nth(0, has_id(&car_2_id)));
    let by_post = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr(&tickets).member("value").eq("Paid"))
        .json_keys(vec![namespace.attribute("tickets")?])
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(by_post.items(), len(eq(1)));
    expect_that!(by_post.items(), nth(0, has_id(&car_2_id)));
    let by_post_2 = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr(&tickets).member("value").eq("Overstay 30 minutes"))
        .json_keys(vec![namespace.attribute("tickets")?])
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(by_post_2.items(), is_empty());
    Ok(())
}
