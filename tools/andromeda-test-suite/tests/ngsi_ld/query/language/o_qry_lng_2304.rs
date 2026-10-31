//! Temporal retrieve and query with an empty language are refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem_among, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{ErrorType, LanguageMap, LanguageProperty, evolution::TemporalEntity};

/// Pins: Retrieve Temporal Evolution of an Entity and Query Temporal Evolution of Entities with an empty lang
/// parameter are refused with 400, `BadRequestData` or `InvalidRequest`
/// Clause: 4.15, 5.7.3, 5.7.4
/// Recorded: clause 4.15 defines lang as a langtag, which an empty value is not, but no clause names the error for
/// it; Table 5.5.2-1 makes `BadRequestData` and `InvalidRequest` both plausible, so both are accepted
#[gtest]
#[tokio::test]
async fn o_qry_lng_2304_temporal_retrieve_and_query_with_an_empty_language_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let street = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place")])?).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&street).send().await?.created()?;
    // The temporal query builders need a timerel, which this read leaves out, and an empty lang is not a language tag,
    // so each call goes out as written.
    let fields = [("Accept", "application/json")];
    for target in [
        format!("temporal/entities/{building_id}?lang="),
        format!("temporal/entities?type=Building_{namespace}&lang="),
    ] {
        let request = exchange(&broker, Method::GET, &target, &fields, None)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
        );
    }
    Ok(())
}
