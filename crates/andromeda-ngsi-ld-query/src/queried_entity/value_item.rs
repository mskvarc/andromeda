//! One value an attribute instance carries, as the `q` evaluator tells values
//! apart (clause 4.9).

use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::time::value::TimeValue;
use andromeda_ngsi_ld_number::JsonNumber;
use std::borrow::Cow;

/// One value held by an attribute instance's family member, or by a member of
/// a structured Property value.
///
/// The value member of each attribute family (clause 5.2.4) holds JSON-LD
/// value objects, node objects and lists. Clause 4.9 compares literals and the
/// IRIs a node names, descends into the members of a structured value, and
/// reads nothing else, which is the distinction this type draws.
#[derive(Debug, Clone)]
pub enum ValueItem<'a, N> {
    /// A JSON-LD value object holding a literal: a Property value, a
    /// `ListProperty` item, the string of one `LanguageProperty` map entry, a
    /// `GeoProperty` geometry written as a string (clause 4.7.2) or a time
    /// value. A language tag is not part of what clause 4.9 compares, and a
    /// datatype only where it declares a time value.
    Literal(LiteralValue<'a>),
    /// A JSON literal (`@type: @json`): a `JsonProperty` value (clause
    /// 4.5.24), or a Property value held as raw JSON outside the JSON-LD graph,
    /// whose keys are addressed verbatim.
    Json(JsonPiece<'a>),
    /// A reference by IRI: a Relationship object, a `VocabProperty` term or a
    /// `ListRelationship` item, as the IRI text clause 4.9 equality compares.
    Reference(&'a str),
    /// A node object: a structured Property value, whose members are addressed
    /// by expanded IRI, or a Property value naming a node by `@id`.
    Node(N),
    /// A value carrying neither a literal nor members a path can address: a
    /// nested list, or a `GeoProperty` geometry held as a geometry.
    Opaque,
}

/// The value of a JSON-LD literal, in the JSON scalar types clause 4.9
/// compares (clause 4.6.3 for numbers) and the time values a literal is
/// declared as (annex C.6).
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue<'a> {
    /// An exact number.
    Number(Cow<'a, JsonNumber>),
    /// A string: a plain or language-tagged string, or the lexical form of a
    /// typed literal declared as no time value.
    Text(Cow<'a, str>),
    /// A boolean.
    Boolean(bool),
    /// A literal declared as a clause 4.6.3 time value, by its datatype or by
    /// the `valueType` of its attribute.
    Time(TimeValue),
}
