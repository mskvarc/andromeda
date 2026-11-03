//! Every write of a member of millions of entries over the entity value budget is 413.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    budget::Budget,
    config::SuiteConfig,
    many_entries::ManyEntries,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::reported,
    problem::{problem, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::BatchOperationResult,
    model::{Entity, Property},
};
use strum::IntoEnumIterator;

/// Pins: Merge Entity, Update Entity Attributes, Append Entity Attributes, Replace Entity, Replace Attribute and
/// Partial Attribute Update of a body whose one `object`, `objectList`, `vocab`, `languageMap` or `scope` holds one
/// entry more than the entity value budget answer 413 `RequestEntityTooLarge`, and a batch create reports the entity
/// with a 413
/// Clause: 6.3.2, 5.6.8
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of an entity outside its
/// geometries and JSON literals (1,000,000), the stack running the defaults, whichever operation writes it
#[gtest]
#[tokio::test]
async fn a_lim_pay_9018_every_write_of_a_member_over_the_entity_value_budget_is_413() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::EntityValue)?;
    let entries = usize::try_from(budget + 1)?;
    let probe_id = namespace.urn("Probe", "operations")?;
    let probe_type = namespace.entity_type("Probe")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .property("m", Property::builder(1))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    for member in ManyEntries::iter() {
        let mut writes = vec![
            (Method::PATCH, format!("entities/{probe_id}"), member.fragment("m", entries)),
            (Method::PUT, format!("entities/{probe_id}"), member.fragment("m", entries)),
        ];
        if let Some(attribute) = member.attribute(entries) {
            writes.push((Method::PATCH, format!("entities/{probe_id}/attrs"), member.fragment("m", entries)));
            writes.push((Method::POST, format!("entities/{probe_id}/attrs"), member.fragment("m", entries)));
            writes.push((Method::PUT, format!("entities/{probe_id}/attrs/m"), attribute.clone())); // clone: the attribute is sent twice
            writes.push((Method::PATCH, format!("entities/{probe_id}/attrs/m"), attribute));
        }
        for (method, path, body) in writes {
            let refused = broker.send_unchecked(json_request_to(&broker, method.clone(), &path, &body)?).await;
            expect_that!(
                &refused,
                problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large()),
                "{method} {path} of {member:?}"
            );
        }
        let batch_id = namespace.urn("Probe", format!("batch-{member:?}").to_lowercase())?;
        let batch = format!("[{}]", member.entity(&batch_id.to_string(), probe_type.as_ref(), "m", entries));
        let answer = broker
            .send_unchecked(json_request_to(&broker, Method::POST, "entityOperations/create", &batch)?)
            .await;
        expect_that!(answer.answered_status(), some(eq(StatusCode::MULTI_STATUS)), "{member:?}");
        let report: BatchOperationResult = reported(answer)?;
        expect_that!(report.success, is_empty(), "{member:?}");
        expect_that!(
            report.errors.iter().map(|failure| failure.error.status).collect::<Vec<_>>(),
            elements_are![some(eq(&413))],
            "{member:?}"
        );
    }
    broker.entities().delete(&probe_id).send().await?.done()?;
    Ok(())
}
