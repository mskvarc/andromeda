use crate::{
    decode_fault::{DeclaredName, DecodeFault},
    fault_recording::{FaultSlot, Recording},
};
use andromeda_ngsi_ld_query::defined_token::{Vocabulary, WrittenWord};
use serde::{
    Deserialize,
    Deserializer,
    de::{DeserializeOwned, Error as DecodeError},
};
use serde_json::{Deserializer as JsonReader, value::RawValue};
use std::{collections::BTreeMap, fmt::Debug};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Inclusive,
    Exclusive,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    #[serde(rename = "type")]
    _kind: String,
    #[serde(default)]
    _mode: Option<Mode>,
    #[serde(default, rename = "triggers")]
    _triggers: Vec<Mode>,
}

/// The failure decoding `text` as a `T` unwatched, and watched with the
/// fault it reports.
fn decoded_watched<T: DeserializeOwned + Debug>(text: &str) -> (serde_json::Error, serde_json::Error, Option<DecodeFault>) {
    let unwatched = serde_json::from_str::<T>(text).expect_err("the fixture must not decode");
    let slot = FaultSlot::default();
    let mut reader = JsonReader::from_str(text);
    let watched = T::deserialize(Recording::new(&mut reader, &slot)).expect_err("the fixture must not decode watched");
    let fault = slot.fault_reported_by(&unwatched);
    (unwatched, watched, fault)
}

#[test]
fn a_value_none_of_the_tokens_is_noted_with_the_tokens_taken() {
    let (_, _, fault) = decoded_watched::<Registration>(r#"{"type": "x", "_mode": "bogus"}"#);
    pretty_assertions::assert_eq!(
        fault,
        Some(DecodeFault::UnknownValue {
            given: WrittenWord::from("bogus".to_owned()),
            takes: Vocabulary(&["inclusive", "exclusive"]),
        })
    );
}

#[test]
fn a_token_inside_an_array_is_noted() {
    let (_, _, fault) = decoded_watched::<Registration>(r#"{"type": "x", "triggers": ["inclusive", "sideways"]}"#);
    assert!(
        matches!(fault, Some(DecodeFault::UnknownValue { ref given, .. }) if *given == WrittenWord::from("sideways".to_owned())),
        "{fault:?}"
    );
}

#[test]
fn a_missing_and_an_unknown_member_are_noted() {
    let (_, _, missing) = decoded_watched::<Registration>(r#"{"_mode": "inclusive"}"#);
    pretty_assertions::assert_eq!(missing, Some(DecodeFault::MissingMember(DeclaredName("type"))));
    let (_, _, unknown) = decoded_watched::<Registration>(r#"{"type": "x", "protcol": 1}"#);
    assert!(
        matches!(unknown, Some(DecodeFault::UnknownMember { ref given, takes }) if *given == WrittenWord::from("protcol".to_owned()) && takes.0.contains(&"type")),
        "{unknown:?}"
    );
}

/// Watching changes nothing the reader returns: its failure is the same, in
/// the same words and at the same place.
#[test]
fn the_reader_returns_the_same_failure_watched() {
    for text in [
        r#"{"type": "x", "_mode": "bogus"}"#,
        r#"{"_mode": "inclusive"}"#,
        r#"{"type": 5}"#,
        r#"{"type": "x", "triggers": "inclusive"}"#,
    ] {
        let (unwatched, watched, _) = decoded_watched::<Registration>(text);
        pretty_assertions::assert_eq!(watched.to_string(), unwatched.to_string(), "{text}");
    }
}

/// A failure the declaration did not raise itself reports no fault.
#[test]
fn a_failure_of_another_kind_reports_no_fault() {
    let (_, _, fault) = decoded_watched::<Registration>(r#"{"type": 5}"#);
    pretty_assertions::assert_eq!(fault, None);
}

/// A type that reads a token and then refuses the whole value in words of
/// its own: the noted fault is not the failure returned, so none is reported.
#[derive(Debug)]
struct Rewording;

impl<'de> Deserialize<'de> for Rewording {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Rewording, D::Error> {
        match Mode::deserialize(deserializer) {
            Ok(_) | Err(_) => Err(DecodeError::custom("refused in words of its own")),
        }
    }
}

#[test]
fn a_fault_that_is_not_the_failure_returned_is_not_reported() {
    let (unwatched, _, fault) = decoded_watched::<Rewording>(r#""bogus""#);
    assert!(unwatched.to_string().starts_with("refused in words of its own"), "{unwatched}");
    pretty_assertions::assert_eq!(fault, None);
}

/// A borrowed string and a raw value are read through the watch as they are
/// read without it.
#[test]
fn borrowed_and_raw_members_are_read_as_written() {
    let text = r#"{"name": "plain", "raw": {"a": [1, null]}}"#;
    let slot = FaultSlot::default();
    let mut reader = JsonReader::from_str(text);
    let read = BTreeMap::<&str, &RawValue>::deserialize(Recording::new(&mut reader, &slot)).expect("the text decodes watched");
    pretty_assertions::assert_eq!(read.get("raw").map(|raw| raw.get()), Some(r#"{"a": [1, null]}"#));
    pretty_assertions::assert_eq!(read.get("name").map(|raw| raw.get()), Some(r#""plain""#));
}
