//! Typed error type for the NGSI-LD query pipeline.
//!
//! Every fallible step in parsing and compiling a query emits a `QueryError`. Variants are strongly-typed: source-bearing causes
//! (`regex::Error`, `serde_json::Error`, `langtag::InvalidLangTag`,
//! `std::num::ParseFloatError`) are chained through `#[source]` / `#[from]`
//! so `.source()` walks the full cause chain.
//!
//! Rendering a compiled query as SQL can fail for reasons of its own -- a bind value with no typed counterpart, a reference geometry that will not encode -- and those belong to the backend that renders it, not here.
//!
//! Faults of a *request* rather than of a language — a `format` value naming no
//! rendering, `drop` and `keep` sent together — belong to the parameter model
//! that reads them, not here. This enum carries only what one of these grammars
//! can itself reject.
//!
//! Mapping onto the ETSI CIM 009 clause 5.5.2 error types lives with the
//! broker's transport-facing error type, not here: structurally malformed
//! inputs become `MalformedRequest`, semantically invalid data becomes
//! `QueryValidation`, and the few faults no client could have caused become
//! internal errors.

use crate::{
    filter::{
        expectation::{QExpectation, QFound},
        json_string::UnicodeEscapeFault,
        path::AttrPath,
        serialize::serialize_attr_path,
    },
    non_posix_construct::NonPosixConstruct,
    ordering::{collation::IcuCollationError, term::OrderDirection},
    scope::pattern_fault::ScopePatternFault,
    term_expander::TermExpansionFailure,
};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRelParseError, geojson::GeometryError, geometry_census::GeometryOverLimit};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::IdentifierParseError,
    ngsi_ld_date::NgsiLdDateError,
    ngsi_ld_date_time::NgsiLdDateTimeError,
    ngsi_ld_time::NgsiLdTimeError,
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_number::JsonNumberError;
use regex_syntax::hir;

/// Errors raised while validating, parsing, compiling or planning a query.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The q expression was empty.
    #[error("q expression: empty input")]
    QEmpty,
    /// A quoted string literal was not closed.
    #[error("q expression: unterminated string literal")]
    QUnterminatedString,
    /// A backslash escape named a character the clause 4.9 `char` production
    /// does not define; IETF RFC 8259 section 7 lists exactly eight.
    #[error("q expression: invalid string escape '\\{escape}' at position {position}")]
    QInvalidStringEscape {
        /// The character that followed the backslash.
        escape: char,
        /// Zero-based character offset of the backslash.
        position: usize,
    },
    /// A `\u` escape did not name a Unicode scalar value.
    #[error("q expression: invalid unicode escape at position {position}: {fault}")]
    QInvalidUnicodeEscape {
        /// Zero-based character offset of the backslash.
        position: usize,
        /// The RFC 8259 rule the escape broke.
        fault: UnicodeEscapeFault,
    },
    /// A quoted string carried a raw control character, which the clause 4.9
    /// `char` production requires to be written as an escape.
    #[error("q expression: unescaped control character U+{code_point:04X} at position {position}")]
    QUnescapedControlCharacter {
        /// The code point that had to be escaped.
        code_point: u32,
        /// Zero-based character offset of the character.
        position: usize,
    },
    /// A token remained after the parser reached end of expression.
    #[error("q expression: unexpected {token} at position {position}")]
    QUnexpectedToken {
        /// Zero-based offset into the token stream.
        position: usize,
        /// What remained.
        token: QFound,
    },
    /// The parser expected a specific token but found something else.
    #[error("q expression: expected {expected}, found {found}")]
    QExpected {
        /// What the grammar required at this position.
        expected: QExpectation,
        /// What stood there instead.
        found: QFound,
    },
    /// The last type of a linked-entity type guard is written bare and holds
    /// two colons, so which one ends the guard cannot be told.
    #[error("q expression: quote the entity type, so the colon ending the type guard is unambiguous")]
    QAmbiguousTypeGuard,
    /// An identifier or character could not be tokenized.
    #[error("q expression: unexpected character '{ch}' at position {position}")]
    QUnexpectedChar {
        /// The character that could not be tokenised.
        ch: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A numeric literal was lexed but failed to parse.
    #[error("q expression: invalid number literal: {0}")]
    QInvalidNumber(#[source] JsonNumberError),
    /// An attribute path nested more sub-attributes than the parser admits.
    ///
    /// A guard against a pathological path whose backend rendering grows with
    /// its depth; the limit is far above any legitimate NGSI-LD nesting.
    #[error("q expression: attribute path too deep ({depth} sub-attributes, limit {limit})")]
    QAttributePathTooDeep {
        /// The number of sub-attributes the path carried.
        depth: usize,
        /// The maximum the parser accepts.
        limit: usize,
    },
    /// A bare literal written in the clause 4.6.3 `DateTime` form was not one.
    #[error("q expression: invalid date-time literal: {0}")]
    QInvalidDateTime(#[source] NgsiLdDateTimeError),
    /// A bare literal written in the clause 4.6.3 `Date` form was not one.
    #[error("q expression: invalid date literal: {0}")]
    QInvalidDate(#[source] NgsiLdDateError),
    /// A bare literal written in the clause 4.6.3 `Time` form was not one.
    #[error("q expression: invalid time literal: {0}")]
    QInvalidTime(#[source] NgsiLdTimeError),

    /// The type expression string was empty.
    #[error("type expression: empty input")]
    TypeExprEmpty,
    /// The type expression ended before a primary could be parsed.
    #[error("type expression: unexpected end of input")]
    TypeExprUnexpectedEnd,
    /// A `)` was expected at the given position but not found.
    #[error("type expression: unbalanced parenthesis at position {0}")]
    TypeExprUnbalanced(usize),
    /// A character outside the type-name grammar was encountered.
    #[error("type expression: unexpected character '{ch}' at position {position}")]
    TypeExprUnexpected {
        /// The offending character.
        ch: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A type name did not satisfy the clause 4.6.2 name grammar.
    #[error("type expression: invalid type name at position {position}: {source}")]
    TypeExprInvalidName {
        /// Zero-based byte offset of the name in the input.
        position: usize,
        /// The clause 4.6.2 rule the name broke.
        #[source]
        source: NgsiLdReferenceError,
    },

    /// A `{` opened without a matching `}`, a `}` closed without a matching
    /// `{`, or content followed a complete term outside the grammar.
    #[error("projection: unbalanced brace at position {position}")]
    ProjectionUnbalancedBrace {
        /// Zero-based byte offset of the offending brace or trailing content.
        position: usize,
    },
    /// A term position (list start, after a separator, or inside `{}`)
    /// produced no attribute name.
    #[error("projection: empty term at position {position}")]
    ProjectionEmptyTerm {
        /// Zero-based byte offset where a name was expected.
        position: usize,
    },
    /// A projection keyword (`id`, `type`, `scope`) carried a clause 4.21
    /// `{...}` linked-entity sub-projection.
    ///
    /// `LinkedEntityTerm` only ever follows `AttrName` in the grammar; the
    /// keywords name structural members of the representation, not
    /// relationships, so they have no linked entity to project.
    #[error("projection: keyword '{keyword}' cannot carry a linked-entity term")]
    ProjectionKeywordWithLinkedTerm {
        /// The offending keyword.
        keyword: AttributeName,
    },

    /// A pattern operand was not a valid regular expression.
    #[error("invalid regular expression: {0}")]
    PatternRegex(#[from] regex::Error),
    /// A repetition operator immediately followed another repetition operator, which PostgreSQL's POSIX engine rejects.
    #[error("invalid POSIX regular expression: a repetition operator cannot follow another repetition operator")]
    PatternRepeatedQuantifier,
    /// The pattern used a construct outside the dialect the broker admits.
    ///
    /// Clause 4.9 mandates IEEE 1003.2. The construct named here is either an extension the broker does not support, or one of the few IEEE 1003.2 constructs the `regex` crate reads differently from POSIX.2; admitting it would make the filter mean something other than what IEEE 1003.2 or the client intended.
    #[error("invalid POSIX regular expression: {construct} is not supported")]
    PatternNonPosixConstruct {
        /// The unsupported construct.
        construct: NonPosixConstruct,
    },
    /// A pattern the `regex` crate compiled could not be translated into the
    /// intermediate representation its store restatement is rendered from.
    #[error("invalid regular expression: {0}")]
    PatternTranslation(#[from] hir::Error),

    /// The `georel` value could not be parsed.
    #[error("geo: {0}")]
    GeoRel(#[from] GeoRelParseError),
    /// Some but not all of the four required geo components were provided.
    #[error("geo: all of georel, geometry, coordinates must be provided together")]
    GeoIncomplete,
    /// The `coordinates` parameter was not valid JSON.
    #[error("geo: coordinates is not valid JSON")]
    GeoCoordinatesInvalid(#[source] serde_json::Error),
    /// The `geometry`/`coordinates` pair did not form a `GeoJSON` geometry.
    #[error("geo-query reference geometry: {0}")]
    ReferenceGeometry(#[from] GeometryError),
    /// The reference geometry of a geo-query or a distance ordering holds
    /// more positions than the broker resolves in one query (clause 5.5.6,
    /// `TooComplexQuery`).
    #[error("the reference geometry holds {over}; this broker resolves no query against it, send a simplified geometry")]
    ReferenceGeometryTooComplex {
        /// The limit the reference geometry holds more than.
        over: GeometryOverLimit,
    },
    /// `georel=near` carried neither `maxDistance` nor `minDistance`.
    #[error("geo: near georel requires maxDistance or minDistance")]
    NearWithoutDistance,

    /// A distance ordering was requested with no geometry to measure from.
    #[error("orderBy: a distance ordering requires orderFrom")]
    DistanceOrderWithoutReference,

    /// A compact term could not be expanded through the active `@context`.
    ///
    /// The expander is a [`TermExpander`](crate::term_expander::TermExpander)
    /// implementation chosen by the caller, so the cause is the expander's own
    /// classification of its failure rather than a type this module would have
    /// to name the JSON-LD layer to hold.
    #[error("could not expand a term: {0}")]
    TermExpansion(#[source] TermExpansionFailure),
    /// A `scopeQ` pattern was outside the clause 4.19 grammar.
    #[error("invalid scopeQ pattern: {0}")]
    ScopePatternInvalid(#[source] ScopePatternFault),
    /// A name was not usable as an attribute name (clause 5.2.4).
    #[error("invalid attribute name: {0}")]
    AttributeNameInvalid(#[source] NgsiLdReferenceError),

    /// The `orderBy` collation identifier was invalid.
    #[error(transparent)]
    IcuCollation(#[from] IcuCollationError),

    /// A temporal query's `orderBy` named an Entity member other than `id`.
    ///
    /// Clause 5.7.4.4: "If the ordering parameter is present and refers an
    /// entity name other than "id", then an error of type `BadRequestData`
    /// shall be raised."
    #[error("a temporal query can only be ordered by the Entity member id, not by '{}'", serialize_attr_path(.0))]
    TemporalOrderingMember(Box<AttrPath>),
    /// A temporal query's `orderBy` asked for a distance ordering.
    ///
    /// Table 6.18.3.2-1 admits only the sorting styles `asc` and `desc`, and
    /// clause 4.23.1 limits sorting by distance to `GeoProperties`, which the
    /// `id` member clause 5.7.4.3 confines the ordering to is not.
    #[error("a temporal query can only be ordered 'asc' or 'desc', not '{0}'")]
    TemporalOrderingDirection(OrderDirection),

    /// `timerel` and `timeAt` must both be present or both absent.
    #[error("timerel and timeAt must both be present or both absent")]
    ParamTimerelTimeatPair,
    /// The `aggrPeriodDuration` value was not an ISO 8601 duration.
    #[error("invalid aggrPeriodDuration value: not an ISO 8601 duration at position {position}")]
    ParamAggrPeriodDurationInvalid {
        /// Zero-based byte offset at which the duration grammar failed.
        position: usize,
    },
    /// A `datasetId` entry was neither `@none` nor a URI short enough to be
    /// an identifier.
    #[error("invalid datasetId value: {0}")]
    ParamDatasetIdInvalid(#[source] IdentifierParseError),
    /// `timerel=between` was provided without an `endTimeAt`.
    #[error("endTimeAt is required when timerel is 'between'")]
    ParamEndTimeAtRequired,
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            expectation::{QExpectation, QFound},
            lexer::TokenKind,
        },
    };

    #[test]
    fn a_missing_token_is_reported_by_kind_or_as_the_end_of_input() {
        let found = QueryError::QExpected {
            expected: QExpectation::Token(TokenKind::RParen),
            found: QFound::Token(TokenKind::Semi),
        };
        pretty_assertions::assert_eq!(found.to_string(), "q expression: expected ), found ;");
        let ended = QueryError::QExpected {
            expected: QExpectation::Value,
            found: QFound::EndOfInput,
        };
        pretty_assertions::assert_eq!(ended.to_string(), "q expression: expected value, found end of input");
    }

    #[test]
    fn a_trailing_token_is_reported_by_kind_and_position() {
        let error = QueryError::QUnexpectedToken {
            position: 3,
            token: QFound::Token(TokenKind::Ident),
        };
        pretty_assertions::assert_eq!(error.to_string(), "q expression: unexpected identifier at position 3");
    }
}
