//! Members a request body may carry and the broker discards unread.
//!
//! Clause 5.2.2 has the broker ignore `createdAt` and `modifiedAt` when a
//! client provides them, and clause 5.8.2.4 does the same for the members the
//! broker generates on a Subscription (Tables 5.2.12-2 and 5.2.14.2-1). A body
//! that denies unknown members cannot simply skip those fields, because a
//! skipped field is not a known one, so each is a `()` field read with
//! [`deserialize`].

use serde::{Deserializer, de::IgnoredAny};

/// Reads any value and keeps none of it.
///
/// # Errors
///
/// Returns the deserializer's error when the input is not well-formed JSON.
pub fn deserialize<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_ignored_any(IgnoredAny).map(|IgnoredAny| ())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Debug, Deserialize, Serialize)]
    struct Holder {
        #[serde(default, skip_serializing, deserialize_with = "crate::ignored_member::deserialize")]
        generated: (),
    }

    #[test]
    fn any_json_value_is_read_and_none_is_written_back() {
        for value in [json!("text"), json!(5), json!(true), json!({"a": [1]}), json!([null]), json!(null)] {
            let holder: Holder = serde_json::from_value(json!({ "generated": value })).expect("every value is ignored");
            let Holder { generated: () } = holder;
            pretty_assertions::assert_eq!(serde_json::to_value(Holder { generated: () }).expect("writes"), json!({}), "{value}");
        }
    }
}
