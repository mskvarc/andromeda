//! The clause 4.6.3 restrictions on `DateTime`, `Date` and `Time` values, and
//! how `q` compares a declared time value and an untyped string.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 fixes three things about a time value:
//! the fraction runs "one or more fractional digits, up to a maximum of six";
//! "in requests, also a comma instead of a decimal point may be used as
//! separator for compatibility reasons"; and a `Time` string "shall not contain
//! expressions of the difference between local time and UTC". A value is a time
//! value only when it is declared as one (annex C.6); a string declared as none
//! is a String.
//!
//! Clause 4.9's grammar defines the query language's `dateTime`, `date` and
//! `time` productions as values "as mandated by clause 4.6.3", unquoted, and
//! makes `quotedStr` a String. A bare offset-bearing operand is therefore
//! refused, a bare operand compares with a declared time value of its form, and
//! a quoted one with a String alone.

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
/// satisfying it: untyped, they are ordinary strings the broker stores and has
/// to be able to find.
const UNADMITTED_TIME_SPELLINGS: [&str; 5] = [
    "2024-01-01T01:00:00+01:00",
    "2024-01-01T00:00:00.1234567Z",
    "12:34:5678",
    "1234-56-78",
    "0000-00-00",
];

/// One observation carrying a Property typed as each clause 4.6.3 time value
/// data type, beside two untyped values that only resemble one.
///
/// `reading_time` and `reading_instant` are what each test varies; the other
/// three are fixed, because they are the same control in every test: a `Date`,
/// a string that is no `DateTime`, and text that is no `Time`.
fn observation(id: &str, reading_time: &str, reading_instant: &str) -> Value {
    json!({
        "id": id,
        "type": "Observation",
        "readingTime": {"type": "Property", "value": {"@type": "Time", "@value": reading_time}},
        "readingInstant": {"type": "Property", "value": {"@type": "DateTime", "@value": reading_instant}},
        "readingDate": {"type": "Property", "value": {"@type": "Date", "@value": READING_DATE}},
        "readingLabel": {"type": "Property", "value": OFFSET_BEARING_LABEL},
        "readingNote": {"type": "Property", "value": COLON_BEARING_NOTE},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An isolated environment holding one observation, and its identifier.
async fn seeded(local: &str, reading_time: &str, reading_instant: &str) -> (TestEnvironment, String) {
    seeded_with(local, |id| observation(id, reading_time, reading_instant)).await
}

/// An isolated environment holding the one entity `entity` builds for its
/// identifier, and that identifier.
async fn seeded_with(local: &str, entity: impl FnOnce(&str) -> Value) -> (TestEnvironment, String) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id(local);
    let created = scope.client().post_entity(&entity(&id)).send().await.expect("create entity failed");
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

/// Assert the bare spelling of one operand, clause 4.9's `dateTime`, `date`
/// or `time`, selects `expected`, and the quoted spelling, a `quotedStr` and so
/// a String, matches the declared time value under no operator but `!=`.
///
/// Clause 4.9: "If there is no equality between the target value data type and
/// the Query Term value data type, then it shall be considered as not
/// matching", and under `unequal` "considered unequal".
async fn assert_bare_and_quoted(scope: &TestEnvironment, id: &str, attribute: &str, operator: &str, operand: &str, expected: Selection) {
    let bare = format!("{attribute}{operator}{operand}");
    match expected {
        Selection::TheEntity => assert_matches(scope, id, &bare).await,
        Selection::Nothing => assert_selects_nothing(scope, id, &bare).await,
    }
    let quoted = format!("{attribute}{operator}\"{operand}\"");
    match operator {
        "!=" => assert_matches(scope, id, &quoted).await,
        // The operators are a closed set of `q` spellings written in this file.
        _other => assert_selects_nothing(scope, id, &quoted).await,
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
/// carry. Resolving `13:00:00+01:00` to `12:00:00Z` and comparing it would let a
/// spelling the write path refuses be queried with.
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
/// refused for the same reason, so the query side admits no operand the storage
/// side rejects.
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
/// operand would make every one of them unqueryable in either spelling.
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

/// Clause 4.9's `quotedStr` is a String whatever separator it carries, so a
/// quoted operand in the comma or the decimal point spelling selects no typed
/// time value, while the bare operand with fewer fraction digits names the
/// same value. A comma cannot be written in a bare operand: it separates the
/// members of a value list.
#[tokio::test]
async fn regression_time_value_a_quoted_operand_is_a_string_whatever_its_separator() {
    let (scope, id) = seeded("Observation:comma-separator-operand", "12:00:00.500000Z", "2024-01-01T00:00:00.250000Z").await;

    for q in [
        "readingTime==\"12:00:00,5Z\"",
        "readingTime==\"12:00:00.5Z\"",
        "readingInstant==\"2024-01-01T00:00:00,25Z\"",
        "readingInstant==\"2024-01-01T00:00:00.25Z\"",
    ] {
        assert_selects_nothing(&scope, &id, q).await;
    }
    for q in ["readingTime==12:00:00.5Z", "readingInstant==2024-01-01T00:00:00.25Z"] {
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

/// The restrictions must not cost the forms clause 4.6.3 does admit. A bare
/// `Z` operand with no fraction, with a short fraction, and with the full six
/// digits names one value, which every one of them finds.
#[tokio::test]
async fn regression_time_value_the_admitted_lexical_forms_still_match() {
    let (scope, id) = seeded("Observation:admitted-forms", "12:00:00Z", "2024-01-01T00:00:00Z").await;

    for q in [
        "readingTime==12:00:00Z",
        "readingTime==12:00:00.0Z",
        "readingTime==12:00:00.000000Z",
        "readingInstant==2024-01-01T00:00:00Z",
        "readingInstant==2024-01-01T00:00:00.000000Z",
    ] {
        assert_matches(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Annex C.6: a string declared as no time value is a String, which the broker
/// holds and serves byte for byte (clause 4.6.4, "return the original
/// content"), whatever clause 4.6.3 form it is written in, an offset-bearing
/// one and free text included.
#[tokio::test]
async fn regression_time_value_an_untyped_string_in_a_time_value_form_is_served_back_as_written() {
    let untyped = |id: &str| {
        json!({
            "id": id,
            "type": "Observation",
            "readingTime": {"type": "Property", "value": "12:00:00.500000Z"},
            "readingInstant": {"type": "Property", "value": "2024-01-01T00:00:00.250000Z"},
            "readingDate": {"type": "Property", "value": READING_DATE},
            "readingLabel": {"type": "Property", "value": OFFSET_BEARING_LABEL},
            "readingNote": {"type": "Property", "value": COLON_BEARING_NOTE},
            "@context": [TEST_SUITE_CONTEXT]
        })
    };
    let (scope, id) = seeded_with("Observation:stored-spelling", untyped).await;

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
/// defines answers over a value declared as a `DateTime`.
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
        assert_bare_and_quoted(&scope, &id, "readingInstant", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The same coverage for clause 4.9's `time` production over a value declared
/// as a `Time`, which is stored in a fixed-width encoding, so the ordering
/// operators are answered byte-wise and are still chronological.
///
/// The bare spelling needs its own tokenisation: `readingTime==09:30:00Z` is not
/// the number `09` followed by an unexpected identifier.
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
        assert_bare_and_quoted(&scope, &id, "readingTime", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's third temporal production, `date`, over a value declared as a
/// `Date`. Clause 4.6.3 gives a `Date` exactly one conformant spelling, so its
/// stored text and every operand written against it are the same fixed-width
/// form and the ordering operators are chronological over it.
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
        assert_bare_and_quoted(&scope, &id, "readingDate", operator, operand, expected).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The control for the three tests above: an untyped value is a JSON string,
/// and is compared as one.
///
/// `readingLabel` is text, so a `dateTime` operand does not match it: clause
/// 4.9 requires that "if there is no equality between the target value data
/// type and the Query Term value data type, then it shall be considered as not
/// matching". A quoted operand is a String and orders against it as text.
///
/// `readingNote` is the other half of the control. Text carrying the colons of a
/// time of day is a `quotedStr` under clause 4.9's grammar, not a malformed
/// `time`; classifying an operand by three byte positions would make
/// `readingNote=="12:00:00 in the morning"` a `BadRequestData` and refuse a
/// conformant filter.
#[tokio::test]
async fn regression_time_value_a_string_that_only_resembles_a_time_value_is_compared_as_text() {
    let (scope, id) = seeded("Observation:resemblance-control", "12:00:00Z", "2024-06-15T12:00:00Z").await;

    assert_matches(&scope, &id, &format!("readingNote==\"{COLON_BEARING_NOTE}\"")).await;
    assert_matches(&scope, &id, "readingNote!=\"09:30:00Z\"").await;

    for q in ["readingLabel==\"2024-01-01T00:00:00Z\"", "readingLabel==2024-01-01T00:00:00Z"] {
        assert_selects_nothing(&scope, &id, q).await;
    }
    for q in ["readingLabel>\"2023-01-01T00:00:00Z\"", "readingLabel<\"2025-01-01T00:00:00Z\""] {
        assert_matches(&scope, &id, q).await;
    }

    scope.finish().await.expect("cleanup failed");
}
