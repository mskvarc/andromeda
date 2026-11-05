//! A member of millions of entries over the entity value budget is 413 even when a body context makes the typed
//! expansion decline the request.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    budget::Budget,
    config::SuiteConfig,
    many_entries::ManyEntries,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::reported,
    problem::{answered, problem, problem_status, request_entity_too_large},
    written_json::ld_json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{BatchOperationResult, headers::Accept},
    model::{Entity, NgsiLdMediaType, Property},
};
use strum::IntoEnumIterator;

/// Pins: Create Entity, Merge Entity and a batch create of a body whose body `@context` aliases `@nest`, which an
/// attribute written first nests a member under, and whose one Property `value`, `object`, `objectList`, `vocab` or
/// `languageMap` holds one entry more than the entity value budget, answer 413 `RequestEntityTooLarge` naming the entity
/// budget and write nothing
/// Clause: 6.3.2, 5.6.8
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of an entity outside its
/// geometries and JSON literals (1,000,000), the stack running the defaults, and counts a request the typed expansion
/// declines from the decoded request before the JSON-LD processor expands it, so a context feature cannot lift the budget
#[gtest]
#[tokio::test]
async fn a_lim_pay_9019_a_member_over_the_entity_value_budget_is_413_when_the_typed_expansion_declines_the_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::EntityValue)?;
    let entries = usize::try_from(budget + 1)?;
    let probe_id = namespace.urn("Probe", "declined")?;
    let probe_type = namespace.entity_type("Probe")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .property("m", Property::builder(1))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    for member in ManyEntries::iter().filter(|member| member.attribute(1).is_some()) {
        let created_id = namespace.urn("Probe", format!("created-{member:?}").to_lowercase())?;
        let body = member.declined_entity(&created_id.to_string(), probe_type.as_ref(), "m", entries);
        let refused = broker.send_unchecked(ld_json_request_to(&broker, Method::POST, "entities", &body)?).await;
        expect_that!(
            &refused,
            problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
                .with_detail_containing(&format!("more than the {budget} JSON values this broker accepts in one entity")),
            "create of {member:?}"
        );
        expect_that!(problem_status(&refused), some(eq(413)), "create of {member:?}");
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&created_id)
            .send()
            .await;
        expect_that!(&absent, answered(StatusCode::NOT_FOUND), "create of {member:?}");
        let merged = broker
            .send_unchecked(ld_json_request_to(
                &broker,
                Method::PATCH,
                &format!("entities/{probe_id}"),
                &member.declined_fragment("m", entries),
            )?)
            .await;
        expect_that!(
            &merged,
            problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large()),
            "merge of {member:?}"
        );
        let batch_id = namespace.urn("Probe", format!("batch-{member:?}").to_lowercase())?;
        let batch = format!("[{}]", member.declined_entity(&batch_id.to_string(), probe_type.as_ref(), "m", entries));
        let answer = broker
            .send_unchecked(ld_json_request_to(&broker, Method::POST, "entityOperations/create", &batch)?)
            .await;
        expect_that!(answer.answered_status(), some(eq(StatusCode::MULTI_STATUS)), "batch of {member:?}");
        let report: BatchOperationResult = reported(answer)?;
        expect_that!(report.success, is_empty(), "batch of {member:?}");
        expect_that!(
            report.errors.iter().map(|failure| failure.error.status).collect::<Vec<_>>(),
            elements_are![some(eq(&413))],
            "batch of {member:?}"
        );
    }
    broker.entities().delete(&probe_id).send().await?.done()?;
    Ok(())
}
