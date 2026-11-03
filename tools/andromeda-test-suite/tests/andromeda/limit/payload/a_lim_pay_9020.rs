//! An entity at the entity value budget is created when a body context makes the typed expansion decline the request.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    budget::Budget,
    config::SuiteConfig,
    many_entries::ManyEntries,
    namespace::Namespace,
    outcome::Done,
    written_json::ld_json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// The values an entity of one Property counts besides its numbers: the entity, its identifier and its type, the
/// attribute, and the instance with its `type`.
const VALUES_BESIDE_THE_NUMBERS: u64 = 6;

/// Pins: Create Entity of an Entity whose one Property holds as many numbers as leave the entity at exactly the entity
/// value budget, under a body `@context` the typed expansion declines, is created, counting the request before the
/// JSON-LD processor expands it refuses nothing the budget admits
/// Clause: 6.3.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of an entity outside its
/// geometries and JSON literals (1,000,000), the stack running the defaults, and counts one value per number of a
/// Property `value`, besides the six the entity, its attribute and its instance hold
#[gtest]
#[tokio::test]
async fn a_lim_pay_9020_an_entity_at_the_entity_value_budget_is_created_when_the_typed_expansion_declines_the_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::EntityValue)?;
    let probe_id = namespace.urn("Probe", "declined-at-the-limit")?;
    let body = ManyEntries::Numbers.declined_entity(
        &probe_id.to_string(),
        &format!("Probe_{namespace}"),
        "m",
        usize::try_from(budget - VALUES_BESIDE_THE_NUMBERS)?,
    );
    let created = broker.send_unchecked(ld_json_request_to(&broker, Method::POST, "entities", &body)?).await;
    expect_that!(created.answered_status(), some(eq(StatusCode::CREATED)));
    broker.entities().delete(&probe_id).send().await?.done()?;
    Ok(())
}
