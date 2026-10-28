//! ETSI `021_15`: the temporal evolutions of the Vehicles are queried by a temporal relation, with `lastN` or without,
//! in normalized or in simplified temporal representation. Each answer is partial content whose `Content-Range` names
//! the time range the body covers, and the instances of the body lie within that range (clauses 5.7.4 and 6.3.10).

use crate::suite_keywords::{ContentRangeText, InstantForm, content_range, create_vehicle_twenty_pair, delete_vehicle_twenty_pair, instants};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    problem::answered,
    wire::json_body,
};
use googletest::prelude::*;
use http::{HeaderMap, StatusCode};
use ngsi_ld::{
    http::{UncheckedRequest, headers::Accept, operation::QueryTemporalEvolution},
    model::Lossless,
    query::{
        SelectorType,
        temporal_query::{self, builders::TemporalQueryBuilder},
    },
};
use serde_json::Value;

/// The `Content-Range` of an answer with `headers`, which names the unit `DateTime` and whose bounds read as NGSI-LD
/// dates.
fn partial_range(headers: &HeaderMap) -> Result<ContentRangeText<'_>> {
    let range = content_range(headers).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));
    expect_that!(range.start, predicate(is_ngsild_date));
    expect_that!(range.end, predicate(is_ngsild_date));
    Ok(range)
}

/// The instants of every instance of `speed` and `fuelLevel` of every entity in `body`, when each instance carries one.
fn body_instants(body: &Value, form: InstantForm) -> Option<Vec<&str>> {
    let lists = body
        .as_array()?
        .iter()
        .flat_map(|entity| ["speed", "fuelLevel"].into_iter().map(move |attribute| instants(entity, attribute, form)))
        .collect::<Option<Vec<_>>>()?;
    Some(lists.into_iter().flatten().collect())
}

/// Checks that `texts` holds `bound` and no instant earlier than it.
fn expect_least_recent(texts: &[&str], bound: &str) {
    let texts = texts.to_vec();
    expect_that!(texts, contains(eq(&bound)));
    expect_that!(texts, each(ge(&bound)));
}

/// Checks that `texts` holds `bound` and no instant later than it.
fn expect_most_recent(texts: &[&str], bound: &str) {
    let texts = texts.to_vec();
    expect_that!(texts, contains(eq(&bound)));
    expect_that!(texts, each(le(&bound)));
}

/// The normalized Query Temporal Evolution of the Vehicles in `relation`, as the typed operation writes it, unsent.
///
/// The typed reader refuses an answer that gives an attribute no instance, so the cases whose partial answer does so
/// read the body as received instead.
fn normalized_request(client: &BrokerClient, relation: TemporalQueryBuilder) -> Result<UncheckedRequest> {
    let temporal_q = relation.build()?;
    let selector: SelectorType = "Vehicle".parse()?;
    let operation = QueryTemporalEvolution::builder()
        .temporal_q(&temporal_q)
        .entity_type(&selector)
        .format(Lossless::Normalized)
        .build();
    Ok(client.unchecked(&operation)?)
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_01_retrieve_the_entity_with_last_n_and_timerel_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::before("2021-01-01T01:01:00Z".parse()?).last_n(20))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = partial_range(received.head.headers())?;
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    expect_that!(range.size, eq("20"));
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_least_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_02_retrieve_the_entity_with_last_n_and_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?).last_n(20))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = partial_range(received.head.headers())?;
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_least_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_03_retrieve_the_entity_with_last_n_and_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2019-01-01T01:01:00Z".parse()?).last_n(20))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = partial_range(received.head.headers())?;
    expect_that!(range.size, eq("20"));
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_most_recent(&texts, range.start);
    expect_least_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_04_retrieve_the_entity_with_timerel_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let answer = client
        .send_unchecked(normalized_request(&client, temporal_query::before("2021-01-01T01:01:00Z".parse()?))?)
        .await;
    verify_that!(&answer, answered(StatusCode::PARTIAL_CONTENT))?;
    let response = answer?;
    let range = partial_range(response.headers())?;
    expect_that!(range.size, eq("*"));
    let body: Value = serde_json::from_slice(response.body())?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_most_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_05_retrieve_the_entity_with_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let answer = client
        .send_unchecked(normalized_request(
            &client,
            temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?),
        )?)
        .await;
    verify_that!(&answer, answered(StatusCode::PARTIAL_CONTENT))?;
    let response = answer?;
    let range = partial_range(response.headers())?;
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body: Value = serde_json::from_slice(response.body())?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_most_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_06_retrieve_the_entity_with_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let answer = client
        .send_unchecked(normalized_request(&client, temporal_query::after("2019-01-01T01:01:00Z".parse()?))?)
        .await;
    verify_that!(&answer, answered(StatusCode::PARTIAL_CONTENT))?;
    let response = answer?;
    let range = partial_range(response.headers())?;
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body: Value = serde_json::from_slice(response.body())?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ObservedAtMember).or_fail()?;
    expect_most_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_07_retrieve_the_entity_with_temporal_values_and_timerel_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2019-01-01T01:01:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = partial_range(received.head.headers())?;
    expect_that!(range.start, eq("2019-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ValuePair).or_fail()?;
    expect_most_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check temporal pagination behavior on multiple entity endpoint
/// Clause: 5.7.4, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
/// Changed: the Content-Range bounds compare with the body's instants as text, RFC 9535 string order in a filter: exact
/// when header and body write instants in the same form, stricter than Robot's Convert Date normalisation otherwise;
/// each bound must also read as an NGSI-LD `DateTime` (`YYYY-MM-DDThh:mm:ss[.fraction]Z`), where Convert Date takes any
/// timestamp it can parse
#[gtest]
#[tokio::test]
async fn e021_15_08_retrieve_the_entity_with_temporal_values_last_n_and_timerel_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::between("2019-01-01T01:01:00Z".parse()?, "2021-01-01T01:01:00Z".parse()?).last_n(20))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = partial_range(received.head.headers())?;
    expect_that!(range.start, eq("2021-01-01T01:01:00Z"));
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(ge(2)));
    let texts = body_instants(&body, InstantForm::ValuePair).or_fail()?;
    expect_least_recent(&texts, range.end);

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}
