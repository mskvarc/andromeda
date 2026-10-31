//! An invalid collation is bad request data on the post binding.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
        temporal_query,
    },
};

/// Pins: Query Temporal Evolution of Entities through POST with ordering collation de-u-nu-latn, de-u-ca-buddhist, "not
/// a collation", und-u-zz-nonsense, de-u-co-nonsense, de-u-ks-zzz or zz-ZZ answers 400 `BadRequestData`, and with
/// und-u-kn-true answers 200
/// Clause: 5.2.23, 5.7.4.4, 6.24.1
/// Recorded: 5.7.4.4 refuses a collation that does not conform to a valid ICU collation (RFC 6067); Andromeda reads a
/// tag whose Unicode extension sets no collation keyword, a collation keyword with an unregistered type, and a language
/// ICU does not know as none
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9021_an_invalid_collation_is_bad_request_data_on_the_post_binding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_collation2_id = namespace.urn("CollationReading", "collation2")?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let collation_reading_collation10_id = namespace.urn("CollationReading", "collation10")?;
    let collation_reading_collation30_id = namespace.urn("CollationReading", "collation30")?;
    for id in [
        &collation_reading_collation2_id,
        &collation_reading_collation10_id,
        &collation_reading_collation30_id,
    ] {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(collation_reading_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for collation in [
        "de-u-nu-latn",
        "de-u-ca-buddhist",
        "not a collation",
        "und-u-zz-nonsense",
        "de-u-co-nonsense",
        "de-u-ks-zzz",
        "zz-ZZ",
    ] {
        // The client refuses the collations the broker must answer 400 for, so the Query is sent as written.
        let body = format!(
            r#"{{"type": "Query", "entities": [{{"type": "CollationReading_{namespace}", "id": ["{collation_reading_collation2_id}", "{collation_reading_collation10_id}", "{collation_reading_collation30_id}"]}}], "temporalQ": {{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}, "ordering": {{"orderBy": ["id"], "collation": "{collation}"}}}}"#
        );
        let request = json_request_to(&client, Method::POST, "temporal/entityOperations/query?local=true", &body)?;
        let answer = client.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let page = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&collation_reading_type)
        .ids(vec![
            collation_reading_collation2_id.clone(),
            collation_reading_collation10_id.clone(),
            collation_reading_collation30_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("id".parse::<OrderBy>()?).collation("und-u-kn-true"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    Ok(())
}
