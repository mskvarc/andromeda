//! ETSI `020_13`: the temporal evolution of an entity with twenty instances on each of two attributes is retrieved in
//! parts. Each part is answered 206 Partial Content with a `Content-Range` that names the time range it covers, and
//! the instances it holds lie within that range (clauses 5.7.3 and 6.3.10).
//!
//! The bounds of the `Content-Range` are compared with the instants of the body as text.

use crate::{
    suite_keywords::{InstantForm, content_range, create_temporal_evolution, instance_list, instants},
    suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_twenty_instances},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    query::temporal_query,
};
use serde_json::Value;

/// The `observedAt` texts of the instances of `speed` and `fuelLevel` in `body`, in that order.
fn instants_of_answer(body: &Value, form: InstantForm) -> Vec<&str> {
    ["speed", "fuelLevel"]
        .into_iter()
        .filter_map(|attribute| instants(body, attribute, form))
        .flatten()
        .collect()
}

/// Checks that `speed` and `fuelLevel` in `body` are lists whose every instance carries an instant.
fn expect_instance_lists(body: &Value, form: InstantForm) {
    for attribute in ["speed", "fuelLevel"] {
        expect_that!(instance_list(body, attribute, form), some(predicate(Value::is_array)));
        expect_that!(instants(body, attribute, form), some(anything()));
    }
}

/// Creates the case's Vehicle as its suite setup does, and answers the vocabulary it is stored under.
async fn created_vehicle(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_twenty_instances(namespace)?).await
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_01_retrieve_an_entity_with_20_instances_on_two_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.start)));
    expect_that!(&texts, each(ge(&range.start)));
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(le(&range.end)));
    expect_that!(range.size, eq("*"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_02_retrieve_the_entity_with_last_n() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .last_n(20)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.start)));
    expect_that!(&texts, each(le(&range.start)));
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(ge(&range.end)));
    expect_that!(range.size, eq("20"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_03_retrieve_the_entity_with_last_n_and_timerel_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::before("2021-01-01T01:01:00Z".parse()?).last_n(20))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(ge(&range.end)));
    expect_that!(range.size, eq("20"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_04_retrieve_the_entity_with_last_n_and_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?).last_n(20))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(ge(&range.end)));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_05_retrieve_the_entity_with_last_n_and_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::after("2019-01-01T01:01:00Z".parse()?).last_n(20))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.start)));
    expect_that!(&texts, each(le(&range.start)));
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(ge(&range.end)));
    expect_that!(range.size, eq("20"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_06_retrieve_the_entity_with_timerel_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::before("2021-01-01T01:01:00Z".parse()?))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(le(&range.end)));
    expect_that!(range.size, eq("*"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_07_retrieve_the_entity_with_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(le(&range.end)));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_08_retrieve_the_entity_with_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .temporal_q(temporal_query::after("2019-01-01T01:01:00Z".parse()?))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ObservedAtMember;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(le(&range.end)));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_09_retrieve_the_entity_with_temporal_values_and_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .temporal_q(temporal_query::after("2019-01-01T01:01:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ValuePair;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(le(&range.end)));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check temporal pagination behavior
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter:
/// exact when header and body write instants in the same form, stricter than Robot's Convert Date normalisation
/// otherwise; each bound must also read as an NGSI-LD `DateTime` (YYYY-MM-DDThh:mm:ss[.fraction]Z), where Convert
/// Date takes any timestamp it can parse
#[gtest]
#[tokio::test]
async fn e020_13_10_retrieve_the_entity_with_temporal_values_last_n_and_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .temporal_q(temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?).last_n(20))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let form = InstantForm::ValuePair;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_instance_lists(&body, form);
    let texts = instants_of_answer(&body, form);
    expect_that!(&texts, contains(eq(&range.end)));
    expect_that!(&texts, each(ge(&range.end)));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
