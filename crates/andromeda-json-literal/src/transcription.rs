//! Writing the JSON value any serde deserializer delivers as JSON text.
//!
//! A literal is read from wherever serde hands it over: a byte reader, a
//! parsed document, or the content serde buffers for a flattened struct or a
//! tagged enum, where `serde_json`'s raw-text capture is unavailable. The
//! value is written as text while it is visited, one level at a time, and is
//! admitted afterwards like any other text; no tree is built.

use crate::LITERAL_CAPTURE;
use andromeda_ngsi_ld_number::{
    JsonNumberError,
    json_value::{READER_NESTING_LIMIT, reader_private_members::SERDE_JSON_PRIVATE_NUMBER_MEMBER},
};
use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use std::fmt;
use thiserror::Error;

/// A value nesting deeper than any JSON text the broker reads.
#[derive(Debug, Error)]
#[error("a JSON literal nests deeper than the {READER_NESTING_LIMIT} levels a JSON text may have")]
struct TooDeep;

/// Write the value `deserializer` delivers as JSON text, asked for under
/// [`LITERAL_CAPTURE`].
///
/// # Errors
///
/// Returns the deserializer's error, or a custom one for a value JSON cannot
/// spell (bytes, a non-finite float, a map key that is not a string) or one
/// nesting deeper than [`READER_NESTING_LIMIT`].
pub(crate) fn transcribed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let mut written = String::new();
    deserializer.deserialize_newtype_struct(
        LITERAL_CAPTURE,
        Transcriber {
            written: &mut written,
            depth: 0,
        },
    )?;
    Ok(written)
}

/// Writes one value, standing `depth` containers deep, into `written`.
struct Transcriber<'w> {
    /// The text written so far.
    written: &'w mut String,
    /// How many containers enclose the value.
    depth: usize,
}

impl Transcriber<'_> {
    /// Write `text` as a JSON string.
    fn string<E: Error>(&mut self, text: &str) -> Result<(), E> {
        let quoted = serde_json::to_string(text).map_err(E::custom)?;
        self.written.push_str(&quoted);
        Ok(())
    }

    /// The transcriber for a value one level deeper.
    ///
    /// # Errors
    ///
    /// Refuses a container standing at the deepest level a JSON text may have.
    fn nested<E: Error>(&mut self) -> Result<Transcriber<'_>, E> {
        if self.depth >= READER_NESTING_LIMIT {
            return Err(E::custom(TooDeep));
        }
        Ok(Transcriber {
            written: self.written,
            depth: self.depth + 1,
        })
    }
}

impl<'de> DeserializeSeed<'de> for Transcriber<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Transcriber<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: Error>(self, flag: bool) -> Result<(), E> {
        self.written.push_str(if flag { "true" } else { "false" });
        Ok(())
    }

    fn visit_i64<E: Error>(self, number: i64) -> Result<(), E> {
        self.written.push_str(&number.to_string());
        Ok(())
    }

    fn visit_u64<E: Error>(self, number: u64) -> Result<(), E> {
        self.written.push_str(&number.to_string());
        Ok(())
    }

    fn visit_i128<E: Error>(self, number: i128) -> Result<(), E> {
        self.written.push_str(&number.to_string());
        Ok(())
    }

    fn visit_u128<E: Error>(self, number: u128) -> Result<(), E> {
        self.written.push_str(&number.to_string());
        Ok(())
    }

    fn visit_f64<E: Error>(self, number: f64) -> Result<(), E> {
        if !number.is_finite() {
            return Err(E::custom(JsonNumberError::NotFinite { value: number }));
        }
        let written = serde_json::to_string(&number).map_err(E::custom)?;
        self.written.push_str(&written);
        Ok(())
    }

    fn visit_str<E: Error>(mut self, text: &str) -> Result<(), E> {
        self.string(text)
    }

    fn visit_unit<E: Error>(self) -> Result<(), E> {
        self.written.push_str("null");
        Ok(())
    }

    fn visit_none<E: Error>(self) -> Result<(), E> {
        self.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<(), A::Error> {
        let nested = self.nested()?;
        nested.written.push('[');
        let mut first = true;
        loop {
            // The separator is written before the element is known to exist,
            // and taken back when the array turns out to have ended.
            let mark = nested.written.len();
            if !first {
                nested.written.push(',');
            }
            let element = Transcriber {
                written: nested.written,
                depth: nested.depth,
            };
            if seq.next_element_seed(element)?.is_none() {
                nested.written.truncate(mark);
                break;
            }
            first = false;
        }
        nested.written.push(']');
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<(), A::Error> {
        let Some(first_name) = map.next_key::<String>()? else {
            self.written.push_str("{}");
            return Ok(());
        };
        if first_name == SERDE_JSON_PRIVATE_NUMBER_MEMBER {
            let token = map.next_value::<String>()?;
            self.written.push_str(&token);
            return Ok(());
        }
        let mut nested = self.nested()?;
        nested.written.push('{');
        let mut name = Some(first_name);
        while let Some(current) = name {
            nested.string(&current)?;
            nested.written.push(':');
            map.next_value_seed(Transcriber {
                written: nested.written,
                depth: nested.depth,
            })?;
            name = map.next_key::<String>()?;
            if name.is_some() {
                nested.written.push(',');
            }
        }
        nested.written.push('}');
        Ok(())
    }
}
