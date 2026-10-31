//! An invalid collation is bad request data on the get binding.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
        temporal_query,
    },
};

/// Pins: Query Temporal Evolution of Entities with orderBy=id and collation de-u-nu-latn, de-u-ca-buddhist, "not a
/// collation", und-u-zz-nonsense, de-u-co-nonsense, de-u-ks-zzz or zz-ZZ answers 400 `BadRequestData`, and with
/// und-u-kn-true answers 200
/// Clause: 5.7.4.4, Table 6.18.3.2-1
/// Recorded: 5.7.4.4 refuses a collation that does not conform to a valid ICU collation (RFC 6067); Andromeda reads a
/// tag whose Unicode extension sets no collation keyword, a collation keyword with an unregistered type, and a language
/// ICU does not know as none
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9020_an_invalid_collation_is_bad_request_data_on_the_get_binding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for collation in [
        "de-u-nu-latn",
        "de-u-ca-buddhist",
        "not%20a%20collation",
        "und-u-zz-nonsense",
        "de-u-co-nonsense",
        "de-u-ks-zzz",
        "zz-ZZ",
    ] {
        // The client refuses the collations the broker must answer 400 for, so the query string is sent as written.
        let request = request_to(
            &client,
            Method::GET,
            &format!(
                "temporal/entities?timerel=after&timeAt=2020-01-01T00:00:00Z&type=CollationReading_{namespace}&local=true&orderBy=id&collation={collation}"
            ),
        )?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = client.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let page = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("id".parse::<OrderBy>()?).collation("und-u-kn-true"))
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
