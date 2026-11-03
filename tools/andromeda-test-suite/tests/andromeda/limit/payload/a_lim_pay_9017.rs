//! An entity whose Relationship holds exactly the entity value budget is created.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    budget::Budget,
    config::SuiteConfig,
    many_entries::ManyEntries,
    namespace::Namespace,
    outcome::Done,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// The values an entity of one Relationship counts besides its targets: the entity, its identifier and its type, the
/// attribute, and the instance with its `type`.
const VALUES_BESIDE_THE_TARGETS: u64 = 6;

/// Pins: Create Entity of an Entity whose one Relationship lists as many targets as leave the entity at exactly the
/// entity value budget is created, counting its targets early refuses nothing the budget admits
/// Clause: 6.3.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of an entity outside its
/// geometries and JSON literals (1,000,000), the stack running the defaults, and counts one value per target of an
/// `object`, besides the six the entity, its attribute and its instance hold
#[gtest]
#[tokio::test]
async fn a_lim_pay_9017_an_entity_at_the_entity_value_budget_held_in_a_relationship_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::EntityValue)?;
    let probe_id = namespace.urn("Probe", "at-the-limit")?;
    let body = ManyEntries::Targets.entity(
        &probe_id.to_string(),
        &format!("Probe_{namespace}"),
        "m",
        usize::try_from(budget - VALUES_BESIDE_THE_TARGETS)?,
    );
    let created = broker.send_unchecked(json_request_to(&broker, Method::POST, "entities", &body)?).await;
    expect_that!(created.answered_status(), some(eq(StatusCode::CREATED)));
    broker.entities().delete(&probe_id).send().await?.done()?;
    Ok(())
}
