//! The operator's own data a broker reports as its `contextSourceExtras`.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml::{Table, Value};

/// The `[federation.identity.extras]` table (ETSI GS CIM 009 v1.9.1 Table
/// 5.2.40-1 `contextSourceExtras`).
///
/// Its content is the operator's: the broker reports it verbatim and never
/// interprets it, so it is held as the table the operator wrote, keys in their
/// written order and spelling. It leaves the configuration only as JSON text,
/// and only when every value has a JSON spelling.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SourceExtras(Table);

/// A value in `[federation.identity.extras]` that JSON cannot spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceExtrasError {
    /// TOML has date-times and JSON does not.
    #[error("federation.identity.extras holds a date-time, which JSON cannot spell; write it as a string")]
    DateTime,
    /// TOML has `inf` and `nan` and JSON does not.
    #[error("federation.identity.extras holds an infinite or not-a-number float, which JSON cannot spell")]
    NonFiniteFloat,
    /// The table could not be written as JSON text.
    #[error("federation.identity.extras could not be written as JSON")]
    Unwritable,
}

impl SourceExtras {
    /// The table as JSON text, members in their written order.
    ///
    /// # Errors
    ///
    /// Returns [`SourceExtrasError::DateTime`] or
    /// [`SourceExtrasError::NonFiniteFloat`] for a value JSON cannot spell, and
    /// [`SourceExtrasError::Unwritable`] when writing fails.
    pub fn to_json_text(&self) -> Result<String, SourceExtrasError> {
        self.0.values().try_for_each(spelled_in_json)?;
        serde_json::to_string(&self.0).map_err(|_| SourceExtrasError::Unwritable)
    }
}

/// Refuse a value, or a value nested in it, that JSON has no spelling for.
fn spelled_in_json(value: &Value) -> Result<(), SourceExtrasError> {
    match value {
        Value::Datetime(_) => Err(SourceExtrasError::DateTime),
        Value::Float(float) if !float.is_finite() => Err(SourceExtrasError::NonFiniteFloat),
        Value::Array(values) => values.iter().try_for_each(spelled_in_json),
        Value::Table(table) => table.values().try_for_each(spelled_in_json),
        Value::String(_) | Value::Integer(_) | Value::Float(_) | Value::Boolean(_) => Ok(()),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::source_extras::{SourceExtras, SourceExtrasError};
    use pretty_assertions::assert_eq;

    fn extras(body: &str) -> SourceExtras {
        toml::from_str(body).expect("a table")
    }

    #[test]
    fn keys_keep_their_spelling_and_written_order() {
        let written = extras("zone = \"eu-west\"\ncontactPoint = \"ops\"\nnodes = [1, 2.5]\n[nested]\nraw = true\n");
        assert_eq!(
            written.to_json_text(),
            Ok(r#"{"zone":"eu-west","contactPoint":"ops","nodes":[1,2.5],"nested":{"raw":true}}"#.to_owned())
        );
    }

    #[test]
    fn a_date_time_anywhere_is_refused() {
        for body in ["at = 1979-05-27T07:32:00Z\n", "[deep]\nlist = [1979-05-27]\n"] {
            assert_eq!(extras(body).to_json_text(), Err(SourceExtrasError::DateTime), "{body}");
        }
    }

    #[test]
    fn an_infinite_or_nan_float_anywhere_is_refused() {
        for body in ["x = inf\n", "x = -inf\n", "[deep]\nx = [nan]\n"] {
            assert_eq!(extras(body).to_json_text(), Err(SourceExtrasError::NonFiniteFloat), "{body}");
        }
    }

    #[test]
    fn the_default_is_an_empty_object() {
        assert_eq!(SourceExtras::default().to_json_text(), Ok("{}".to_owned()));
    }
}
