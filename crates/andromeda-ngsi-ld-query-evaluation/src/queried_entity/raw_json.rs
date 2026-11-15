//! Raw JSON a value holds, and what a JSON-LD value object inside it is.
//!
//! A JSON literal (`@type: @json`, clause 4.5.24) stays outside the JSON-LD
//! graph, so an object inside it is the object it is, whatever members it
//! carries. A registration's Context Source Property is JSON-LD its registrant
//! wrote and the broker keeps as written, so a value object inside it is a
//! JSON-LD typed value (clause 3.1, annex C.6) wherever it sits: the property's
//! value, an element of an array value, or a member of an object value at any
//! depth.

use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::time::value::{TimeValue, TimeValueForm};
use andromeda_ngsi_ld_vocabulary::keyword;

/// What a JSON-LD value object inside raw JSON is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueObjects {
    /// An object like any other.
    Untyped,
    /// The time value it declares when its `@type` names a clause 4.6.3 time
    /// value, by the core `@context` term or by the IRI, and its `@value`
    /// spells one in the clause 4.6.3 form of that datatype; an object like
    /// any other otherwise.
    Typed,
}

/// Raw JSON a value holds, addressed by its verbatim keys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawJson<'a> {
    /// The JSON.
    piece: JsonPiece<'a>,
    /// What a value object inside it is.
    value_objects: ValueObjects,
}

impl<'a> RawJson<'a> {
    /// The JSON of a JSON literal.
    #[must_use]
    pub const fn json_literal(piece: JsonPiece<'a>) -> RawJson<'a> {
        RawJson {
            piece,
            value_objects: ValueObjects::Untyped,
        }
    }

    /// JSON-LD kept as its writer wrote it.
    #[must_use]
    pub const fn written_json_ld(piece: JsonPiece<'a>) -> RawJson<'a> {
        RawJson {
            piece,
            value_objects: ValueObjects::Typed,
        }
    }

    /// The JSON.
    #[must_use]
    pub const fn piece(self) -> JsonPiece<'a> {
        self.piece
    }

    /// The elements of a JSON array, each read as this JSON is; `None` for any
    /// other JSON.
    #[must_use]
    pub fn elements(self) -> Option<Vec<RawJson<'a>>> {
        let elements = self.piece.elements()?;
        Some(elements.into_iter().map(|piece| RawJson { piece, ..self }).collect())
    }

    /// The member a path of verbatim keys reaches, read as this JSON is.
    #[must_use]
    pub fn member_path<'n, I: IntoIterator<Item = &'n str>>(self, names: I) -> Option<RawJson<'a>> {
        Some(RawJson {
            piece: self.piece.member_path(names)?,
            ..self
        })
    }

    /// The time value the JSON is: a value object declaring one when value
    /// objects are typed values here, and whose `@value` its declared form
    /// admits; `None` for any other JSON.
    #[must_use]
    pub fn declared_time(self) -> Option<TimeValue> {
        match self.value_objects {
            ValueObjects::Untyped => None,
            ValueObjects::Typed => {
                let form = TimeValueForm::declared_by(&self.piece.member(keyword::TYPE)?.as_str()?)?;
                form.parse(&self.piece.member(keyword::VALUE)?.as_str()?).ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::queried_entity::raw_json::RawJson;
    use andromeda_json_literal::JsonLiteral;

    fn literal(text: &str) -> JsonLiteral {
        JsonLiteral::admit(text).expect("a literal")
    }

    /// A value object is a typed value in JSON-LD kept as written, by its
    /// datatype's term or IRI and only in the datatype's clause 4.6.3 form,
    /// and the object it is in a JSON literal.
    #[test]
    fn a_value_object_declares_a_time_value_only_in_json_ld_and_in_its_form() {
        for (json, expected) in [
            (r#"{"@type": "DateTime", "@value": "2024-01-01T00:00:00Z"}"#, Some("2024-01-01T00:00:00Z")),
            (r#"{"@type": "https://uri.etsi.org/ngsi-ld/Date", "@value": "2024-01-01"}"#, Some("2024-01-01")),
            (r#"{"@type": "Time", "@value": "08:00:00,5Z"}"#, Some("08:00:00.500Z")),
            (r#"{"@type": "DateTime", "@value": "2024-01-01T00:00:00+00:00"}"#, None),
            (r#"{"@type": "Date", "@value": "2024-02-30"}"#, None),
            (r#"{"@type": "Text", "@value": "2024-01-01"}"#, None),
            (r#"{"@value": "2024-01-01T00:00:00Z"}"#, None),
            (r#""2024-01-01T00:00:00Z""#, None),
        ] {
            let json = literal(json);
            pretty_assertions::assert_eq!(
                RawJson::written_json_ld(json.piece()).declared_time().map(|time| time.to_string()).as_deref(),
                expected,
                "{}",
                json.piece().text()
            );
            assert!(RawJson::json_literal(json.piece()).declared_time().is_none(), "{}", json.piece().text());
        }
    }

    /// An element and a member keep the reading of the JSON they are found in.
    #[test]
    fn an_element_and_a_member_are_read_as_the_json_holding_them() {
        let json = literal(r#"{"list": [{"@type": "DateTime", "@value": "2024-01-01T00:00:00Z"}], "held": {"m": {"@type": "Date", "@value": "2024-01-01"}}}"#);
        let declared = |raw: RawJson<'_>| {
            let element = raw
                .member_path(["list"])
                .and_then(RawJson::elements)
                .and_then(|elements| elements.first().copied());
            let member = raw.member_path(["held", "m"]);
            (
                element.and_then(RawJson::declared_time).is_some(),
                member.and_then(RawJson::declared_time).is_some(),
            )
        };
        pretty_assertions::assert_eq!(declared(RawJson::written_json_ld(json.piece())), (true, true));
        pretty_assertions::assert_eq!(declared(RawJson::json_literal(json.piece())), (false, false));
    }
}
