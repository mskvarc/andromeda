//! A query of context source registrations with a temporal query is matched against the registration's
//! `observationInterval` or `managementInterval` (clause 5.10.2).

use crate::registration_operations::{create_registration, csource_endpoint, delete_registrations};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    query::{temporal_query, temporal_query::TemporalProperty},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo, TimeInterval},
};
use serde_json::json;

/// The interval member the registration of a case carries.
#[derive(Debug, Clone, Copy)]
enum IntervalMember {
    /// `observationInterval`.
    Observation,
    /// `managementInterval`.
    Management,
}

impl IntervalMember {
    /// The member name.
    const fn name(self) -> &'static str {
        match self {
            IntervalMember::Observation => "observationInterval",
            IntervalMember::Management => "managementInterval",
        }
    }
}

/// Registers the vocabulary and the registration `-1` of every `Building` whose `member` runs from
/// 2020-08-01T22:07:00Z to 2021-08-01T21:07:00Z, queries the registrations of `Building` before 2021-08-01T22:00:00Z
/// on `time_property` (none named when `None`), requires 200 and the registration alone, and deletes it.
async fn queried(member: IntervalMember, time_property: Option<TemporalProperty>) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let interval = TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?);
    let builder = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?]);
    let registration = match member {
        IntervalMember::Observation => builder.observation_interval(interval).build()?,
        IntervalMember::Management => builder.management_interval(interval).build()?,
    };
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;

    let before = temporal_query::before("2021-08-01T22:00:00Z".parse()?);
    let temporal_q = match time_property {
        Some(property) => before.time_property(property),
        None => before,
    };
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Building")
        .temporal_q(temporal_q)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([{
        "id": namespace.urn_text("ContextSourceRegistration", 1),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        (member.name()): {"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"},
        "endpoint": endpoint.as_str(),
    }]);
    expect_that!(&body, matches_document(expected).hiding_everywhere("mode").hiding_everywhere("operations"));
    delete_registrations(&broker, &namespace, &[1]).await
}

/// Pins: Check that one can query context source registrations. If present, the temporal query is matched against the
/// observationInterval or the managementInterval
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_09_01_observation_interval_with_observed_at() -> Result<()> {
    queried(IntervalMember::Observation, Some(TemporalProperty::ObservedAt)).await
}

/// Pins: Check that one can query context source registrations. If present, the temporal query is matched against the
/// observationInterval or the managementInterval
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_09_02_observation_interval_without_timeproperty() -> Result<()> {
    queried(IntervalMember::Observation, None).await
}

/// Pins: Check that one can query context source registrations. If present, the temporal query is matched against the
/// observationInterval or the managementInterval
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_09_03_management_interval_with_created_at() -> Result<()> {
    queried(IntervalMember::Management, Some(TemporalProperty::CreatedAt)).await
}

/// Pins: Check that one can query context source registrations. If present, the temporal query is matched against the
/// observationInterval or the managementInterval
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_09_04_management_interval_with_modified_at() -> Result<()> {
    queried(IntervalMember::Management, Some(TemporalProperty::ModifiedAt)).await
}
