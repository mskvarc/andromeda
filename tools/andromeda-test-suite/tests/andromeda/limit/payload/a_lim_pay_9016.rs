//! A member of millions of entries over the entity value budget is 413 and creates nothing, whichever member holds them.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    many_entries::ManyEntries,
    namespace::Namespace,
    problem::{answered, problem, problem_status, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};
use strum::IntoEnumIterator;

/// Pins: Create Entity of a body whose one `object`, `objectList`, `vocab`, `languageMap` or `scope` holds one entry
/// more than the entity value budget, far under the request value budget, answers 413 `RequestEntityTooLarge` naming
/// the entity budget and creates nothing
/// Clause: 6.3.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of a request (4,000,000) and
/// of an entity outside its geometries and JSON literals (1,000,000), the stack running the defaults, and counts the
/// entries of each of these members before it expands them, so such a body is refused before it is expanded whole
#[gtest]
#[tokio::test]
async fn a_lim_pay_9016_a_member_of_millions_of_entries_over_the_entity_value_budget_is_413_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::EntityValue)?;
    let entries = usize::try_from(budget + 1)?;
    for member in ManyEntries::iter() {
        let probe_id = namespace.urn("Probe", format!("{member:?}").to_lowercase())?;
        let body = member.entity(&probe_id.to_string(), &format!("Probe_{namespace}"), "m", entries);
        let refused = broker.send_unchecked(json_request_to(&broker, Method::POST, "entities", &body)?).await;
        expect_that!(
            &refused,
            problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
                .with_detail_containing(&format!("more than the {budget} JSON values this broker accepts in one entity")),
            "{member:?}"
        );
        expect_that!(problem_status(&refused), some(eq(413)), "{member:?}");
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&probe_id)
            .send()
            .await;
        expect_that!(&absent, answered(StatusCode::NOT_FOUND), "{member:?}");
    }
    Ok(())
}
