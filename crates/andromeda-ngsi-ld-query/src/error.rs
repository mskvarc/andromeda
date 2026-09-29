//! Typed error type for the NGSI-LD query pipeline.
//!
//! Every fallible step in parsing and compiling a query emits a `QueryError`. Variants are strongly-typed: source-bearing causes
//! (`regex::Error`, `serde_json::Error`, `langtag::InvalidLangTag`,
//! `std::num::ParseFloatError`) are chained through `#[source]` / `#[from]`
//! so `.source()` walks the full cause chain.
//!
//! Rendering a compiled query as SQL can fail for reasons of its own — a bind
//! value with no typed counterpart, a reference geometry that will not encode —
//! and those belong to the backend that renders it, not here. See
//! `andromeda_postgresql_query::error::RenderError`.
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
    filter::json_string::UnicodeEscapeFault,
    ordering::{collation::IcuCollationError, term::OrderDirection},
};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRelParseError, geojson::GeometryError};
use andromeda_ngsi_ld_lexicon::{
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
    // -----------------------------------------------------------------------
    // Q expression grammar (NGSI-LD 4.9)
    // -----------------------------------------------------------------------
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
    #[error("q expression: unexpected token at position {position}: {token}")]
    QUnexpectedToken {
        /// Zero-based offset into the token stream.
        position: usize,
        /// Debug rendering of the offending token.
        token: String,
    },
    /// The parser expected a specific token but found something else.
    #[error("q expression: expected {expected}, found {found}")]
    QExpected {
        /// Human-readable description of what was expected.
        expected: String,
        /// Debug rendering of what was encountered instead.
        found: String,
    },
    /// An identifier or character could not be tokenized.
    #[error("q expression: unexpected character '{ch}' at position {position}")]
    QUnexpectedChar {
        /// The character that could not be tokenised.
        ch: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A numeric literal was lexed but failed to parse.
    #[error("q expression: invalid number literal {value}: {source}")]
    QInvalidNumber {
        /// The rejected numeric operand.
        value: String,
        /// The exact-number syntax or supported-range failure.
        #[source]
        source: JsonNumberError,
    },
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
    #[error("q expression: invalid date-time literal '{value}'")]
    QInvalidDateTime {
        /// The rejected literal.
        value: String,
        /// The clause 4.6.3 rule the literal broke.
        #[source]
        source: NgsiLdDateTimeError,
    },
    /// A bare literal written in the clause 4.6.3 `Date` form was not one.
    #[error("q expression: invalid date literal '{value}'")]
    QInvalidDate {
        /// The rejected literal.
        value: String,
        /// The clause 4.6.3 rule the literal broke.
        #[source]
        source: NgsiLdDateError,
    },
    /// A bare literal written in the clause 4.6.3 `Time` form was not one.
    #[error("q expression: invalid time literal '{value}'")]
    QInvalidTime {
        /// The rejected literal.
        value: String,
        /// The clause 4.6.3 rule the literal broke.
        #[source]
        source: NgsiLdTimeError,
    },

    // -----------------------------------------------------------------------
    // Type expression grammar (NGSI-LD 4.17)
    // -----------------------------------------------------------------------
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
    /// A type name production produced an empty name.
    #[error("type expression: empty type name at position {0}")]
    TypeExprEmptyName(usize),
    /// A type name did not satisfy the clause 4.6.2 name grammar.
    #[error("type expression: invalid type name '{name}' at position {position}")]
    TypeExprInvalidName {
        /// The offending name.
        name: String,
        /// Zero-based byte offset of the name in the input.
        position: usize,
    },

    // -----------------------------------------------------------------------
    // Raw projection grammar (clause 4.21 `pick`/`omit` linked-entity syntax)
    // -----------------------------------------------------------------------
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
        keyword: String,
    },

    // -----------------------------------------------------------------------
    // Regular expressions (clause 4.9 `q` patterns, clause 5.7.2 `idPattern`)
    // -----------------------------------------------------------------------
    /// A pattern operand was not a valid regular expression.
    #[error("invalid regular expression: {0}")]
    PatternRegex(#[from] regex::Error),
    /// A repetition operator immediately followed another repetition operator, which PostgreSQL's POSIX engine rejects.
    #[error("invalid POSIX regular expression: a repetition operator cannot follow another repetition operator")]
    PatternRepeatedQuantifier,
    /// The pattern used a construct outside the dialect the broker admits.
    ///
    /// Clause 4.9 mandates IEEE 1003.2. The construct named here is either an
    /// extension the broker does not support, or one of the few IEEE 1003.2
    /// constructs the `regex` crate reads differently from POSIX.2; admitting
    /// it would make the filter mean something other than what IEEE 1003.2 or
    /// the client intended. `crate::posix_dialect` names each case.
    #[error("invalid POSIX regular expression: {construct} is not supported")]
    PatternNonPosixConstruct {
        /// How the unsupported construct is spelled.
        construct: &'static str,
    },
    /// A pattern the `regex` crate compiled could not be translated into the
    /// intermediate representation its store restatement is rendered from.
    #[error("invalid regular expression: {0}")]
    PatternTranslation(#[from] hir::Error),

    // -----------------------------------------------------------------------
    // Geo query (NGSI-LD 4.10)
    // -----------------------------------------------------------------------
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
    /// `georel=near` carried neither `maxDistance` nor `minDistance`.
    #[error("geo: near georel requires maxDistance or minDistance")]
    NearWithoutDistance,

    // -----------------------------------------------------------------------
    // Ordering (NGSI-LD 4.23)
    // -----------------------------------------------------------------------
    /// A distance ordering was requested with no geometry to measure from.
    #[error("orderBy: distance ordering requires either a geo-query or an orderFrom parameter")]
    DistanceOrderWithoutReference,

    // -----------------------------------------------------------------------
    // Term expansion
    // -----------------------------------------------------------------------
    /// A compact term could not be expanded through the active `@context`.
    ///
    /// The cause is deliberately carried as text: the expander is a
    /// [`TermExpander`](crate::term_expander::TermExpander)
    /// implementation chosen by the caller, so its failure type is not
    /// knowable here without this module naming the JSON-LD layer.
    #[error("could not expand term '{term}': {message}")]
    TermExpansion {
        /// The compact term that failed to expand.
        term: String,
        /// Human-readable rendering of the expander's own failure.
        message: String,
    },
    /// A `scopeQ` pattern was outside the clause 4.19 grammar.
    #[error("invalid scopeQ pattern: '{0}'")]
    ScopePatternInvalid(String),
    /// An expanded IRI was not usable as an attribute name (clause 5.2.4).
    #[error("'{value}' is not a valid attribute name")]
    AttributeNameInvalid {
        /// The rejected value.
        value: String,
        /// The underlying grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// A term expanded to something that is not an absolute IRI.
    ///
    /// NGSI-LD requires the core `@context`, whose `@vocab` resolves every
    /// unmapped term, so this means the active context is broken rather than
    /// that the client sent something unusual.
    #[error("term '{term}' expanded to '{expanded}', which is not an absolute IRI")]
    TermNotAnIri {
        /// The compact term that was expanded.
        term: String,
        /// What it expanded to.
        expanded: String,
    },

    // -----------------------------------------------------------------------
    // ICU collation (NGSI-LD 4.23)
    // -----------------------------------------------------------------------
    /// The `orderBy` collation identifier was invalid.
    #[error(transparent)]
    IcuCollation(#[from] IcuCollationError),

    // -----------------------------------------------------------------------
    // Temporal entity ordering (NGSI-LD 5.7.4.3, Table 6.18.3.2-1)
    // -----------------------------------------------------------------------
    /// A temporal query's `orderBy` named an Entity member other than `id`.
    ///
    /// Clause 5.7.4.4: "If the ordering parameter is present and refers an
    /// entity name other than "id", then an error of type `BadRequestData`
    /// shall be raised."
    #[error("a temporal query can only be ordered by the Entity member id, not by '{0}'")]
    TemporalOrderingMember(String),
    /// A temporal query's `orderBy` asked for a distance ordering.
    ///
    /// Table 6.18.3.2-1 admits only the sorting styles `asc` and `desc`, and
    /// clause 4.23.1 limits sorting by distance to `GeoProperties`, which the
    /// `id` member clause 5.7.4.3 confines the ordering to is not.
    #[error("a temporal query can only be ordered 'asc' or 'desc', not '{0}'")]
    TemporalOrderingDirection(OrderDirection),

    // -----------------------------------------------------------------------
    // Parameter values the query and projection languages above parse themselves (clauses 4.9 and 4.21)
    // -----------------------------------------------------------------------
    /// An `id` value could not be parsed as a URI, or is one too long to be
    /// an identifier.
    #[error("invalid id value '{raw}': {source}")]
    ParamIdInvalid {
        /// The value as the request spelled it.
        raw: String,
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// An empty value appeared in a comma-separated id list.
    #[error("invalid id: empty value in comma-separated list")]
    ParamIdListEmpty,
    /// `timerel` and `timeAt` must both be present or both absent.
    #[error("timerel and timeAt must both be present or both absent")]
    ParamTimerelTimeatPair,
    /// The `aggrPeriodDuration` value was not an ISO 8601 duration.
    #[error("invalid aggrPeriodDuration value: '{0}'")]
    ParamAggrPeriodDurationInvalid(String),
    /// A `datasetId` entry was neither `@none` nor a URI short enough to be
    /// an identifier.
    #[error("invalid datasetId value '{raw}': {source}")]
    ParamDatasetIdInvalid {
        /// The value as the request spelled it.
        raw: String,
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// `timerel=between` was provided without an `endTimeAt`.
    #[error("endTimeAt is required when timerel is 'between'")]
    ParamEndTimeAtRequired,
}
