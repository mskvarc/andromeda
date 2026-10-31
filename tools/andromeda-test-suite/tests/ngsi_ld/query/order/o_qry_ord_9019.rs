//! A locale stating no collation preference is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: `orderBy=name` with collation=de-u-nu-latn or collation=und-u-zz-nonsense answers 400 `BadRequestData`
/// Clause: 5.7.2.4
/// Recorded: 5.7.2.4 refuses a collation that does not conform to a valid ICU collation (RFC 6067); Andromeda reads
/// a tag whose Unicode extension sets no collation keyword (co, ks and the other collation keys) as none
#[gtest]
#[tokio::test]
async fn o_qry_ord_9019_a_locale_stating_no_collation_preference_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Building")?)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("name".parse::<OrderBy>()?).collation("de-u-nu-latn"))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Building")?)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("name".parse::<OrderBy>()?).collation("und-u-zz-nonsense"))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Building")?)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("name".parse::<OrderBy>()?).collation("und-x-icu"))
        .send()
        .await?;
    Ok(())
}
