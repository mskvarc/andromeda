//! The `coordinates` member of a compact `GeoJSON` geometry, recorded before
//! its nesting is known.
//!
//! A geometry object's members come in any order, so the `type` that decides
//! how `coordinates` nests (IETF RFC 7946 sections 3.1.2 to 3.1.7) may follow
//! the coordinates it governs. The member is therefore recorded first, without
//! regard to its shape, as a flat tape: one byte for every array boundary and
//! every member, and one double for every ordinate. A two-dimensional position
//! costs four bytes beside its two ordinates, a fraction of what a JSON tree
//! spends on it, which allocates every array and every number token.
//!
//! An ordinate is recorded as the double nearest to the number written, so a
//! number no finite double stands for is recorded as a member that is not an
//! ordinate at all.

use andromeda_ngsi_ld_number::json_value::value_census::{MemberName, MemberNameSeed};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Number;
use std::{fmt, slice::Iter};

/// One step of a recorded `coordinates` member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoordinateStep {
    /// An array opens.
    Open,
    /// The innermost open array closes.
    Close,
    /// A number a finite double stands for; the double is the next one the
    /// tape recorded.
    Ordinate,
    /// Any other member: a string, a boolean, null, an object, or a number no
    /// finite double stands for.
    Other,
}

/// A `coordinates` member, recorded in document order.
#[derive(Debug, Default)]
pub(crate) struct CoordinateTape {
    /// Every array boundary and member, in document order.
    steps: Vec<CoordinateStep>,
    /// The double of every [`CoordinateStep::Ordinate`], in the same order.
    ordinates: Vec<f64>,
}

/// One item read back from a [`CoordinateTape`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TapeItem {
    /// An array opens.
    Open,
    /// The innermost open array closes.
    Close,
    /// A number, as the double nearest to it.
    Ordinate(f64),
    /// A member that is neither an array nor a number a finite double stands
    /// for.
    Other,
}

impl CoordinateTape {
    /// Record a `coordinates` member written as JSON text.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` failure for text that is no single JSON value.
    pub(crate) fn of_text(text: &str) -> Result<CoordinateTape, serde_json::Error> {
        let mut tape = CoordinateTape::default();
        let mut reader = serde_json::Deserializer::from_str(text);
        CoordinateTapeSeed::new(&mut tape).deserialize(&mut reader)?;
        reader.end()?;
        Ok(tape)
    }

    /// Record a number by the double that stands for it, if a finite one
    /// does.
    fn record_number(&mut self, ordinate: Option<f64>) {
        match ordinate.filter(|ordinate| ordinate.is_finite()) {
            Some(ordinate) => {
                self.steps.push(CoordinateStep::Ordinate);
                self.ordinates.push(ordinate);
            }
            None => self.steps.push(CoordinateStep::Other),
        }
    }

    /// Read the tape back from its first step.
    pub(crate) fn reader(&self) -> TapeReader<'_> {
        TapeReader {
            steps: self.steps.iter(),
            ordinates: self.ordinates.iter(),
        }
    }
}

/// Reads a [`CoordinateTape`] back, one item at a time.
#[derive(Debug)]
pub(crate) struct TapeReader<'t> {
    /// The steps not read yet.
    steps: Iter<'t, CoordinateStep>,
    /// The ordinates not read yet.
    ordinates: Iter<'t, f64>,
}

impl TapeReader<'_> {
    /// The next item, or `None` past the last one.
    ///
    /// Every recorded ordinate step has its double beside it; a step without
    /// one reads as a member that is no ordinate.
    pub(crate) fn next_item(&mut self) -> Option<TapeItem> {
        self.steps.next().map(|step| match step {
            CoordinateStep::Open => TapeItem::Open,
            CoordinateStep::Close => TapeItem::Close,
            CoordinateStep::Ordinate => self.ordinates.next().map_or(TapeItem::Other, |ordinate| TapeItem::Ordinate(*ordinate)),
            CoordinateStep::Other => TapeItem::Other,
        })
    }

    /// Whether the next item closes the innermost open array, consuming it if
    /// it does.
    ///
    /// The end of the tape closes every array still open.
    pub(crate) fn closes(&mut self) -> bool {
        match self.steps.as_slice().first() {
            Some(CoordinateStep::Close) => {
                self.steps.next();
                true
            }
            Some(CoordinateStep::Open | CoordinateStep::Ordinate | CoordinateStep::Other) => false,
            None => true,
        }
    }
}

/// Records one value read from a deserializer, and everything below it, onto
/// a tape.
#[derive(Debug)]
pub(crate) struct CoordinateTapeSeed<'t> {
    /// The tape the value is recorded onto.
    tape: &'t mut CoordinateTape,
}

impl CoordinateTapeSeed<'_> {
    /// A seed recording onto `tape`.
    pub(crate) const fn new(tape: &mut CoordinateTape) -> CoordinateTapeSeed<'_> {
        CoordinateTapeSeed { tape }
    }
}

impl<'de> DeserializeSeed<'de> for CoordinateTapeSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CoordinateTapeSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeoJSON coordinates")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<(), E> {
        self.tape.steps.push(CoordinateStep::Other);
        Ok(())
    }

    // An integer reads as the double `serde_json::Value::as_f64` gives for
    // it, the double nearest to it.
    fn visit_i64<E: DeserializeError>(self, value: i64) -> Result<(), E> {
        self.tape.record_number(Number::from(value).as_f64());
        Ok(())
    }

    fn visit_u64<E: DeserializeError>(self, value: u64) -> Result<(), E> {
        self.tape.record_number(Number::from(value).as_f64());
        Ok(())
    }

    fn visit_i128<E: DeserializeError>(self, value: i128) -> Result<(), E> {
        self.tape.record_number(Number::from_i128(value).and_then(|number| number.as_f64()));
        Ok(())
    }

    fn visit_u128<E: DeserializeError>(self, value: u128) -> Result<(), E> {
        self.tape.record_number(Number::from_u128(value).and_then(|number| number.as_f64()));
        Ok(())
    }

    fn visit_f64<E: DeserializeError>(self, value: f64) -> Result<(), E> {
        self.tape.record_number(Some(value));
        Ok(())
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<(), E> {
        self.tape.steps.push(CoordinateStep::Other);
        Ok(())
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        self.tape.steps.push(CoordinateStep::Other);
        Ok(())
    }

    fn visit_none<E: DeserializeError>(self) -> Result<(), E> {
        self.tape.steps.push(CoordinateStep::Other);
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        self.tape.steps.push(CoordinateStep::Open);
        while items.next_element_seed(CoordinateTapeSeed { tape: &mut *self.tape })?.is_some() {}
        self.tape.steps.push(CoordinateStep::Close);
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        match members.next_key_seed(MemberNameSeed)? {
            // The reader's spelling of a number that is not a 64-bit integer.
            Some(MemberName::NumberToken) => {
                let ordinate = members.next_value_seed(OrdinateTokenSeed)?;
                self.tape.record_number(ordinate);
            }
            Some(MemberName::Member) => {
                members.next_value::<IgnoredAny>()?;
                while members.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                self.tape.steps.push(CoordinateStep::Other);
            }
            None => self.tape.steps.push(CoordinateStep::Other),
        }
        Ok(())
    }
}

/// Reads the text of a number token as the double nearest to it.
#[derive(Clone, Copy, Debug)]
struct OrdinateTokenSeed;

impl<'de> DeserializeSeed<'de> for OrdinateTokenSeed {
    type Value = Option<f64>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<f64>, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for OrdinateTokenSeed {
    type Value = Option<f64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a number token")
    }

    fn visit_str<E: DeserializeError>(self, token: &str) -> Result<Option<f64>, E> {
        Ok(token.parse::<f64>().ok())
    }
}

#[cfg(test)]
mod tests {
    use crate::coordinate_tape::{CoordinateTape, CoordinateTapeSeed, TapeItem};
    use serde::de::DeserializeSeed;
    use serde_json::Value;
    use std::iter::from_fn;

    /// Every item of `tape`, in order.
    fn items(tape: &CoordinateTape) -> Vec<TapeItem> {
        let mut reader = tape.reader();
        from_fn(|| reader.next_item()).collect()
    }

    /// The tape `text` records when read from its text.
    fn streamed(text: &str) -> CoordinateTape {
        CoordinateTape::of_text(text).expect("valid JSON")
    }

    /// The tape a parsed tree records when it is read through serde.
    fn of_tree(tree: &Value) -> CoordinateTape {
        let mut tape = CoordinateTape::default();
        CoordinateTapeSeed::new(&mut tape).deserialize(tree).expect("a tree is read to its end");
        tape
    }

    #[test]
    fn text_with_trailing_characters_or_no_value_records_no_tape() {
        assert!(CoordinateTape::of_text("[1, 2] x").is_err());
        assert!(CoordinateTape::of_text("[1, ").is_err());
    }

    #[test]
    fn a_text_and_its_tree_record_the_same_tape() {
        for text in [
            "[[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0, 12.5], [0.0, 0.0]]]",
            "[14.550000000000000001, 46.05, 1e400, -0, 18446744073709551616]",
            r#"[[1, "x"], {"a": 1}, null, true, [], [[]]]"#,
            r#"{"type": "Point"}"#,
            "7",
        ] {
            let tree: Value = serde_json::from_str(text).expect("valid JSON");
            pretty_assertions::assert_eq!(items(&streamed(text)), items(&of_tree(&tree)), "{text}");
        }
    }

    #[test]
    fn a_number_no_finite_double_stands_for_is_recorded_as_no_ordinate() {
        pretty_assertions::assert_eq!(
            items(&streamed("[1e400, 2]")),
            vec![TapeItem::Open, TapeItem::Other, TapeItem::Ordinate(2.0), TapeItem::Close]
        );
    }

    /// An arbitrary-precision decimal reads as the double nearest to it.
    #[test]
    fn an_arbitrary_precision_ordinate_is_recorded_as_its_nearest_double() {
        pretty_assertions::assert_eq!(items(&streamed("14.550000000000000001")), vec![TapeItem::Ordinate(14.55)]);
    }
}
