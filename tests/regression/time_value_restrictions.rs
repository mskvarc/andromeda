//! The clause 4.6.3 restrictions on `DateTime` and `Time` lexical values.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 fixes three things about a temporal
//! literal: the fraction runs "one or more fractional digits, up to a maximum
//! of six"; "in requests, also a comma instead of a decimal point may be used
//! as separator for compatibility reasons"; and a `Time` string "shall not
//! contain expressions of the difference between local time and UTC".
//!
//! Clause 4.9's grammar defines the query language's `dateTime` and `time`
//! productions as values "as mandated by clause 4.6.3", so a `q` operand is
//! admitted on exactly the terms a stored value is. The broker did the mirror
//! image of the clause on two counts: it resolved an offset-bearing operand to
//! UTC and compared it, and it refused the comma compatibility form outright.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The calendar date every observation carries, in the clause 4.6.3 `Date` form.
const READING_DATE: &str = "2024-06-15";

/// A value carrying the punctuation of a clause 4.6.3 `DateTime` without being
/// one: the clause requires that the trailing timestamp component "shall always
/// be equal to the character 'Z'", so a numeric offset leaves this an ordinary
/// JSON string, which is one of "all the JSON native data types" the same clause
/// admits.
const OFFSET_BEARING_LABEL: &str = "2024-01-01T01:00:00+01:00";

/// Free-form text that happens to carry the colons of a time of day. Clause
/// 4.9's grammar makes it a `quotedStr`, which compares as the text it is.
const COLON_BEARING_NOTE: &str = "12:00:00 in the morning";

/// Values written in one of clause 4.6.3's three lexical forms without
/// satisfying it, which the same clause therefore leaves as "JSON native data
/// types" — ordinary strings the broker stores and has to be able to find.
const UNADMITTED_TIME_SPELLINGS: [&str; 5] = [
    "2024-01-01T01:00:00+01:00",
    "2024-01-01T00:00:00.1234567Z",
    "12:34:5678",
    "1234-56-78",
    "0000-00-00",
];

/// One observation carrying a Property of each clause 4.6.3 time value form,
/// beside two values that only resemble one.
///
/// `reading_time` and `reading_instant` are what each test varies; the other
/// three are fixed, because they are the same control in every test: a `Date`,
/// a string a broker must not read as a `DateTime`, and text a broker must not
/// read as a `Time`.
fn observation(id: &str, reading_time: &str, reading_instant: &str) -> Value {
    json!({
        "id": id,
        "type": "Observation",
        "readingTime": {"type": "Property", "value": reading_time},
        "readingInstant": {"type": "Property", "value": reading_instant},
        "readingDate": {"type": "Property", "value": READING_DATE},
        "readingLabel": {"type": "Property", "value": OFFSET_BEARING_LABEL},
        "readingNote": {"type": "Property", "value": COLON_BEARING_NOTE},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An isolated environment holding one observation, and its identifier.
async fn seeded(local: &str, reading_time: &str, reading_instant: &str) -> (TestEnvironment, String) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id(local);
    let created = scope
        .client()
        .post_entity(&observation(&id, reading_time, reading_instant))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    (scope, id)
}

/// Run one `q` filter against the seeded entity alone and return the response.
///
/// The query names the entity by `id`, so the answer is about this entity and
/// no concurrently running suite can contribute a row.
async fn query(scope: &TestEnvironment, id: &str, q: &str) -> TestResponse {
    let request = format!("id={}&q={}", urlencoding::encode(id), urlencoding::encode(q));
    scope
        .client()
        .query_entities(&request, "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("entity query failed")
}

/// Assert a `q` filter is refused as the clause 5.5.4 `BadRequestData`.
async fn assert_refused(scope: &TestEnvironment, id: &str, q: &str) {
    let response = query(scope, id, q).await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "q={q}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "q={q}: {problem}");
}

/// Assert a `q` filter is accepted and selects the seeded entity.
async fn assert_matches(scope: &TestEnvironment, id: &str, q: &str) {
    let response = query(scope, id, q).await;
    assert_eq!(response.status, StatusCode::OK, "q={q}: {:?}", response.body);
    let body = response.json_value().expect("query response must be JSON");
    let entities = body.as_array().expect("query response must be an array");
    assert_eq!(entities.len(), 1, "q={q} did not select the seeded entity: {body}");
}

/// Assert a `q` filter is accepted and selects nothing.
///
/// Clause 4.9 answers a Query Term the target does not satisfy — including one
/// whose data type differs from the operand's — by not matching, which is a
/// selection of none rather than a refusal.
async fn assert_selects_nothing(scope: &TestEnvironment, id: &str, q: &str) {
    let response = query(scope, id, q).await;
    assert_eq!(response.status, StatusCode::OK, "q={q}: {:?}", response.body);
    let body = response.json_value().expect("query response must be JSON");
    let entities = body.as_array().expect("query response must be an array");
    assert_eq!(entities.len(), 0, "q={q} selected the seeded entity: {body}");
}

/// Assert both spellings of one operand — the bare literal clause 4.9's
/// `dateTime`, `date` and `time` productions define, and the quoted form its
/// `quotedStr` production admits — reach the same answer.
///
/// Clause 4.9 gives no rule making a quoted value a String: its own EXAMPLE 3,
/// `?q=isParked=="urn:ngsi-ld:OffStreetParking:Downtown1"`, quotes a URI and
/// requires it to match a Relationship object, and EXAMPLE 11 quotes
/// `"Overstay 60 minutes"` because the value carries spaces. Quoting is the
/// escaping device, and the value's own lexical form is its data type.
async fn assert_both_spellings(scope: &TestEnvironment, id: &str, attribute: &str, operator: &str, operand: &str, expected: Selection) {
    for spelling in [format!("{attribute}{operator}{operand}"), format!("{attribute}{operator}\"{operand}\"")] {
        match expected {
            Selection::TheEntity => assert_matches(scope, id, &spelling).await,
            Selection::Nothing => assert_selects_nothing(scope, id, &spelling).await,
        }
    }
}

/// What a Query Term is expected to select from the seeded scope.
#[derive(Debug, Clone, Copy)]
enum Selection {
    /// The one seeded entity.
    TheEntity,
    /// No entity at all.
    Nothing,
}

/// Clause 4.6.3, `Time`: "The string shall not contain expressions of the
/// difference between local time and UTC. All representations shall be
/// interpreted as being expressed in UTC."
///
/// A numeric offset is exactly such an expression, so a *bare* `time` operand
/// carrying one satisfies clause 4.9's `time` production nowhere and is
/// derivable from no other production either — `quotedStr` is the IETF RFC 8259
/// `String` production *including its quotes*, which a bare literal does not
/// carry. The broker used to resolve `13:00:00+01:00` to `12:00:00Z` and compare
/// it, so a spelling the write path refuses could still be queried with.
///
/// The quoted spelling of the same text is a `quotedStr` and therefore a
/// well-formed Query Term: it compares as the text it is, which is exactly what
/// the write path stored for a value written that way.
#[tokio::test]
async fn regression_time_value_a_bare_q_time_operand_carrying_a_numeric_offset_is_refused() {
    let (scope, id) = seeded("Observation:time-offset-operand", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    for q in ["readingTime==13:00:00+01:00", "readingTime>09:00:00-05:00", "readingTime==12:00:00+00:00"] {
        assert_refused(&scope, &id, q).await;
    }

    for q in [
        "readingTime==\"13:00:00+01:00\"",
        "readingTime==\"12:00:00+00:00\"",
        "readingTime==\"12:00:00.500000Z is the reading\"",
    ] {
        assert_selects_nothing(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.3, `DateTime`, requires the literal `Z` designator in the same
/// way, so a bare `q` `dateTime` operand written with a numeric offset is
/// refused for the same reason. Without this the query side admitted operands
/// the storage side rejects, and the two sides disagreed about what a value is.
///
/// Quoted, the same text is a `quotedStr` and compares as text — against
/// `readingLabel`, which holds exactly one such value, it matches.
#[tokio::test]
async fn regression_time_value_a_bare_q_date_time_operand_carrying_a_numeric_offset_is_refused() {
    let (scope, id) = seeded("Observation:instant-offset-operand", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    for q in [
        "readingInstant==2024-01-01T01:00:00+01:00",
        "readingInstant>2023-12-31T19:00:00-05:00",
        "readingInstant==2024-01-01T00:00:00+00:00",
    ] {
        assert_refused(&scope, &id, q).await;
    }

    for q in ["readingInstant==\"2024-01-01T01:00:00+01:00\"", "readingInstant==\"2024-01-01T00:00:00+00:00\""] {
        assert_selects_nothing(&scope, &id, q).await;
    }
    assert_matches(&scope, &id, &format!("readingLabel==\"{OFFSET_BEARING_LABEL}\"")).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's `ComparableValue` production is `Number / quotedStr / dateTime /
/// date / time`, and `quotedStr = String` is the IETF RFC 8259 `String`
/// production, which carries its own quotation marks — the clause writes its
/// `dateTime` operands unquoted, as EXAMPLE 8's
/// `?q=temperature.observedAt>=2017-12-24T12:00:00Z` shows. A quoted literal is
/// therefore derivable whatever it contains, and the broker has no grounds to
/// refuse one.
///
/// It also has every reason not to. Each value below is one clause 4.6.3 admits
/// as "a JSON native data type" and stores as the string it is, and refusing the
/// operand made every one of them unqueryable in either spelling: bare it was a
/// malformed `dateTime`, `date` or `time`, and quoted it was the same refusal.
/// A broker that accepts a value and then answers 400 to the only filter that
/// names it has lost the value.
#[tokio::test]
async fn regression_time_value_a_quoted_operand_no_temporal_form_admits_finds_the_value_it_names() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Observation:quoted-text-operands");

    let mut entity = json!({
        "id": &id,
        "type": "Observation",
        "@context": [TEST_SUITE_CONTEXT]
    });
    let members = entity.as_object_mut().expect("the entity is a JSON object");
    for (index, value) in UNADMITTED_TIME_SPELLINGS.iter().enumerate() {
        members.insert(format!("reading{index}"), json!({"type": "Property", "value": value}));
    }
    let created = scope.client().post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for (index, value) in UNADMITTED_TIME_SPELLINGS.iter().enumerate() {
        let attribute = format!("reading{index}");
        assert_matches(&scope, &id, &format!("{attribute}==\"{value}\"")).await;
        assert_selects_nothing(&scope, &id, &format!("{attribute}!=\"{value}\"")).await;
        assert_refused(&scope, &id, &format!("{attribute}=={value}")).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.3, for both lexical forms: "In requests, also a comma instead of
/// a decimal point may be used as separator for compatibility reasons."
///
/// A `q` operand is request text, so the comma form has to name the same value
/// the decimal point does. The broker refused it, which made a conformant
/// client's filter a 400.
#[tokio::test]
async fn regression_time_value_a_q_operand_may_use_a_comma_decimal_separator() {
    let (scope, id) = seeded("Observation:comma-separator-operand", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    for q in [
        "readingTime==\"12:00:00,5Z\"",
        "readingTime==\"12:00:00.5Z\"",
        "readingInstant==\"2024-01-01T00:00:00,25Z\"",
        "readingInstant==\"2024-01-01T00:00:00.25Z\"",
    ] {
        assert_matches(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.3: the fraction runs "one or more fractional digits, up to a
/// maximum of six". A seventh digit puts the literal outside the lexical form,
/// so a bare operand carrying one is malformed rather than silently truncated
/// into one that matches a stored value it was never equal to. Quoted, it is a
/// `quotedStr` and compares as text, which is what the write path stored for a
/// value written that way.
#[tokio::test]
async fn regression_time_value_a_bare_q_operand_beyond_six_fractional_digits_is_refused() {
    let (scope, id) = seeded("Observation:excess-fraction-operand", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    for q in ["readingTime==12:00:00.1234567Z", "readingInstant==2024-01-01T00:00:00.1234567Z"] {
        assert_refused(&scope, &id, q).await;
    }

    for q in ["readingTime==\"12:00:00.1234567Z\"", "readingInstant==\"2024-01-01T00:00:00.1234567Z\""] {
        assert_selects_nothing(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The tightening must not cost the forms clause 4.6.3 does admit. A `Z` value
/// with no fraction, with a short fraction, and with the full six digits are
/// all one value, and all remain queryable.
#[tokio::test]
async fn regression_time_value_the_admitted_lexical_forms_still_match() {
    let (scope, id) = seeded("Observation:admitted-forms", "12:00:00Z", "2024-01-01T00:00:00Z").await;

    for q in [
        "readingTime==\"12:00:00Z\"",
        "readingTime==\"12:00:00.0Z\"",
        "readingTime==\"12:00:00.000000Z\"",
        "readingInstant==\"2024-01-01T00:00:00Z\"",
        "readingInstant==\"2024-01-01T00:00:00.000000Z\"",
    ] {
        assert_matches(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.3 read on the storage side rather than the query side: a value a
/// client wrote is served back as written. This is the control for the operand
/// tests above -- if the write path silently rewrote the stored spelling, a
/// matching query would prove nothing about the operand.
///
/// Clause 4.6.3 admits several conformant spellings of one value -- a fraction
/// of "one or more fractional digits, up to a maximum of six" -- and licenses no
/// broker to substitute one for another, so the document carries the client's
/// text. What the value *is* under the clause, and therefore what a `q` operand
/// is compared against, is decided separately and never reaches the document.
/// The broker used to sniff the value into a typed one for the document alone
/// and re-render it, which cost `2024-01-01T00:00:00.250000Z` its trailing
/// zeroes and rewrote `2024-01-01T01:00:00+01:00` -- a value clause 4.6.3 makes
/// an ordinary string, because the trailing component "shall always be equal to
/// the character 'Z'" -- into a UTC instant the client never wrote.
#[tokio::test]
async fn regression_time_value_a_stored_time_property_is_served_back_verbatim() {
    let (scope, id) = seeded("Observation:stored-spelling", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    let read = scope
        .client()
        .get_entity(&id, "", "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("read the entity back");
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    let body = read.json_value().expect("an entity body");
    assert_eq!(body["readingTime"]["value"], json!("12:00:00.500000Z"), "{body}");
    assert_eq!(body["readingInstant"]["value"], json!("2024-01-01T00:00:00.250000Z"), "{body}");
    assert_eq!(body["readingDate"]["value"], json!(READING_DATE), "{body}");
    assert_eq!(body["readingLabel"]["value"], json!(OFFSET_BEARING_LABEL), "{body}");
    assert_eq!(body["readingNote"]["value"], json!(COLON_BEARING_NOTE), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's `ComparableValue` admits a `dateTime`, and clause 4.8 NOTE 3
/// puts a time value on an ordinary Property, so every operator the clause
/// defines has to answer over one. None of them did: the write path read a
/// timestamp-shaped Property value as a String and the operand parser read the
/// operand as a `dateTime`, and clause 4.9 makes differing data types
/// non-matching -- so equality, inequality and all four ordering operators
/// selected nothing, in every spelling.
///
/// "When comparing dates or times, the order relation considered shall be a
/// temporal one", which is why `12:00:00.000000Z` and `12:00:00Z` name the same
/// instant on both sides of a comparison.
#[tokio::test]
async fn regression_time_value_a_date_time_property_answers_every_clause_4_9_operator() {
    let (scope, id) = seeded("Observation:instant-operators", "12:00:00Z", "2024-06-15T12:00:00Z").await;

    for (operator, operand, expected) in [
        ("==", "2024-06-15T12:00:00Z", Selection::TheEntity),
        ("==", "2024-06-15T12:00:00.000000Z", Selection::TheEntity),
        ("==", "2024-06-15T12:00:01Z", Selection::Nothing),
        ("!=", "2024-06-15T12:00:01Z", Selection::TheEntity),
        ("!=", "2024-06-15T12:00:00Z", Selection::Nothing),
        (">", "2024-06-15T11:59:59Z", Selection::TheEntity),
        (">", "2024-06-15T12:00:00Z", Selection::Nothing),
        (">=", "2024-06-15T12:00:00Z", Selection::TheEntity),
        (">=", "2024-06-15T12:00:01Z", Selection::Nothing),
        ("<", "2024-06-15T12:00:01Z", Selection::TheEntity),
        ("<", "2024-06-15T12:00:00Z", Selection::Nothing),
        ("<=", "2024-06-15T12:00:00Z", Selection::TheEntity),
        ("<=", "2024-06-15T11:59:59Z", Selection::Nothing),
    ] {
        assert_both_spellings(&scope, &id, "readingInstant", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The same coverage for clause 4.9's `time` production. A `Time` is compared
/// through the canonical fixed-width rendering clause 4.6.3's own restrictions
/// make possible, so the ordering operators are answered byte-wise and are
/// still chronological; a `Time` Property that reached storage in the spelling
/// the client wrote would break exactly that.
///
/// The bare spelling had no tokenisation at all: `readingTime==09:30:00Z` was
/// read as the number `09` followed by an unexpected identifier and refused.
#[tokio::test]
async fn regression_time_value_a_time_property_answers_every_clause_4_9_operator() {
    let (scope, id) = seeded("Observation:time-operators", "09:30:00Z", "2024-06-15T12:00:00Z").await;

    for (operator, operand, expected) in [
        ("==", "09:30:00Z", Selection::TheEntity),
        ("==", "09:30:00.000000Z", Selection::TheEntity),
        ("==", "09:30:01Z", Selection::Nothing),
        ("!=", "09:30:01Z", Selection::TheEntity),
        ("!=", "09:30:00Z", Selection::Nothing),
        (">", "09:29:59Z", Selection::TheEntity),
        (">", "09:30:00Z", Selection::Nothing),
        (">=", "09:30:00Z", Selection::TheEntity),
        (">=", "09:30:01Z", Selection::Nothing),
        ("<", "09:30:01Z", Selection::TheEntity),
        ("<", "09:30:00Z", Selection::Nothing),
        ("<=", "09:30:00Z", Selection::TheEntity),
        ("<=", "09:29:59Z", Selection::Nothing),
    ] {
        assert_both_spellings(&scope, &id, "readingTime", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's third temporal production, `date`, over the same classifier.
/// Clause 4.6.3 gives a `Date` exactly one conformant spelling, so its stored
/// text and every operand written against it are the same fixed-width form and
/// the ordering operators are chronological over it.
#[tokio::test]
async fn regression_time_value_a_date_property_answers_every_clause_4_9_operator() {
    let (scope, id) = seeded("Observation:date-operators", "12:00:00Z", "2024-06-15T12:00:00Z").await;

    for (operator, operand, expected) in [
        ("==", "2024-06-15", Selection::TheEntity),
        ("==", "2024-06-16", Selection::Nothing),
        ("!=", "2024-06-16", Selection::TheEntity),
        ("!=", "2024-06-15", Selection::Nothing),
        (">", "2024-06-14", Selection::TheEntity),
        (">", "2024-06-15", Selection::Nothing),
        (">=", "2024-06-15", Selection::TheEntity),
        (">=", "2024-06-16", Selection::Nothing),
        ("<", "2024-06-16", Selection::TheEntity),
        ("<", "2024-06-15", Selection::Nothing),
        ("<=", "2024-06-15", Selection::TheEntity),
        ("<=", "2024-06-14", Selection::Nothing),
    ] {
        assert_both_spellings(&scope, &id, "readingDate", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The control for the three tests above: a value that only *resembles* a time
/// value is a JSON string, and is compared as one.
///
/// Clause 4.6.3 admits "all the JSON native data types" beside its three time
/// value forms, and a string is only one of those forms when it satisfies the
/// form completely -- a numeric offset where the clause demands `Z` does not.
/// So `readingLabel` stays text, and a `dateTime` operand does not match it:
/// clause 4.9 requires that "if there is no equality between the target value
/// data type and the Query Term value data type, then it shall be considered as
/// not matching".
///
/// `readingNote` is the other half of the control. Text carrying the colons of a
/// time of day is a `quotedStr` under clause 4.9's grammar, not a malformed
/// `time`; classifying an operand by three byte positions made
/// `readingNote=="12:00:00 in the morning"` a `BadRequestData` and refused a
/// conformant filter outright.
#[tokio::test]
async fn regression_time_value_a_string_that_only_resembles_a_time_value_is_compared_as_text() {
    let (scope, id) = seeded("Observation:resemblance-control", "12:00:00Z", "2024-06-15T12:00:00Z").await;

    assert_matches(&scope, &id, &format!("readingNote==\"{COLON_BEARING_NOTE}\"")).await;
    assert_matches(&scope, &id, "readingNote!=\"09:30:00Z\"").await;

    for q in [
        "readingLabel==\"2024-01-01T00:00:00Z\"",
        "readingLabel==2024-01-01T00:00:00Z",
        "readingLabel>\"2023-01-01T00:00:00Z\"",
        "readingLabel<\"2025-01-01T00:00:00Z\"",
    ] {
        assert_selects_nothing(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}
