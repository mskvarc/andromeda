//! A discovery q naming a full language tag is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Context Source Registrations with q label`[en-GB]`=="Lift" answers 400 `BadRequestData` naming the
/// tag, and with q label`[en]`=="Lift" answers 200
/// Clause: 4.9, 5.10.2.4
/// Recorded: the clause 4.9 `AttrName` a bracket holds admits no `-`, so a full tag is outside the grammar clause
/// 5.10.2.4 holds a discovery query to; the stack runs the specification level of `q_target_languages`
#[gtest]
#[tokio::test]
async fn n_csr_qry_9016_a_discovery_q_naming_a_full_language_tag_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let label = namespace.attribute("label")?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .q(q::attr(&label).member("en-GB").eq("Lift"))
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("en-GB")
    );
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .q(q::attr(&label).member("en").eq("Lift"))
        .send()
        .await?;
    Ok(())
}
