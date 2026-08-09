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
//! Mapping onto the ETSI CIM 009 clause 5.6.3 error types lives with the
//! broker's transport-facing error type, not here: structurally malformed
//! inputs become `MalformedRequest`, semantically invalid data becomes
//! `QueryValidation`, and the few faults no client could have caused become
//! internal errors.

use crate::ordering::collation::IcuCollationError;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRelParseError, geometry::GeometryError};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;

/// Errors raised while validating, parsing, compiling or planning a query.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    // -----------------------------------------------------------------------
    // Q expression grammar (NGSI-LD 4.9)
    // -----------------------------------------------------------------------
    /// The q expression was empty.
    #[error("Q expression: empty input")]
    QEmpty,
    /// A quoted string literal was not closed.
    #[error("Q expression: unterminated string literal")]
    QUnterminatedString,
    /// A token remained after the parser reached end of expression.
    #[error("Q expression: unexpected token at position {position}: {token}")]
    QUnexpectedToken {
        /// Zero-based offset into the token stream.
        position: usize,
        /// Debug rendering of the offending token.
        token: String,
    },
    /// The parser expected a specific token but found something else.
    #[error("Q expression: expected {expected}, found {found}")]
    QExpected {
        /// Human-readable description of what was expected.
        expected: String,
        /// Debug rendering of what was encountered instead.
        found: String,
    },
    /// An identifier or character could not be tokenized.
    #[error("Q expression: unexpected character '{ch}' at position {position}")]
    QUnexpectedChar {
        /// The character that could not be tokenised.
        ch: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A numeric literal was lexed but failed to parse.
    #[error("Q expression: invalid number literal {0}")]
    QInvalidNumber(String),
    /// An attribute path nested more sub-attributes than the parser admits.
    ///
    /// A guard against a pathological path whose backend rendering grows with
    /// its depth; the limit is far above any legitimate NGSI-LD nesting.
    #[error("Q expression: attribute path too deep ({depth} sub-attributes, limit {limit})")]
    QAttributePathTooDeep {
        /// The number of sub-attributes the path carried.
        depth: usize,
        /// The maximum the parser accepts.
        limit: usize,
    },
    /// A literal shaped like an RFC 3339 date-time was not one.
    #[error("Q expression: invalid date-time literal '{value}'")]
    QInvalidDateTime {
        /// The rejected literal.
        value: String,
        /// The underlying chrono parse error.
        #[source]
        source: chrono::ParseError,
    },
    /// A literal shaped like an RFC 3339 full-date was not one.
    #[error("Q expression: invalid date literal '{value}'")]
    QInvalidDate {
        /// The rejected literal.
        value: String,
        /// The underlying chrono parse error.
        #[source]
        source: chrono::ParseError,
    },
    /// A literal shaped like an RFC 3339 full-time was not one.
    #[error("Q expression: invalid time literal '{value}'")]
    QInvalidTime {
        /// The rejected literal.
        value: String,
        /// The underlying chrono parse error.
        #[source]
        source: chrono::ParseError,
    },

    // -----------------------------------------------------------------------
    // Type expression grammar (NGSI-LD 4.17)
    // -----------------------------------------------------------------------
    /// The type expression string was empty.
    #[error("Type expression: empty input")]
    TypeExprEmpty,
    /// The type expression ended before a primary could be parsed.
    #[error("Type expression: unexpected end of input")]
    TypeExprUnexpectedEnd,
    /// A `)` was expected at the given position but not found.
    #[error("Type expression: unbalanced parenthesis at position {0}")]
    TypeExprUnbalanced(usize),
    /// A character outside the type-name grammar was encountered.
    #[error("Type expression: unexpected character {ch:?} at position {position}")]
    TypeExprUnexpected {
        /// The offending character.
        ch: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A type name production produced an empty name.
    #[error("Type expression: empty type name at position {0}")]
    TypeExprEmptyName(usize),
    /// A type name did not satisfy the clause 4.6.2 name grammar.
    #[error("Type expression: invalid type name {name:?} at position {position}")]
    TypeExprInvalidName {
        /// The offending name.
        name: String,
        /// Zero-based byte offset of the name in the input.
        position: usize,
    },

    // -----------------------------------------------------------------------
    // Regular expressions (clause 4.9 `q` patterns, clause 5.7.2 `idPattern`)
    // -----------------------------------------------------------------------
    /// A pattern operand was not a valid regular expression.
    #[error("invalid regular expression")]
    PatternRegex(#[from] regex::Error),
    /// A repetition operator immediately followed another repetition operator, which PostgreSQL's POSIX engine rejects.
    #[error("invalid POSIX regular expression: a repetition operator cannot follow another repetition operator")]
    PatternRepeatedQuantifier,

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
    /// The `geometry`/`coordinates` pair did not form a GeoJSON geometry.
    #[error("geo: invalid reference geometry")]
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
    #[error("Invalid scopeQ pattern: '{0}'")]
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
    // Parameter values the languages above parse themselves (clause 5.6.3)
    // -----------------------------------------------------------------------
    /// An `id` value could not be parsed as a URI.
    #[error("Invalid id value: '{0}'")]
    ParamIdInvalid(String),
    /// An empty value appeared in a comma-separated id list.
    #[error("Invalid id: empty value in comma-separated list")]
    ParamIdListEmpty,
    /// `timerel` and `timeAt` must both be present or both absent.
    #[error("timerel and timeAt must both be present or both absent")]
    ParamTimerelTimeatPair,
    /// The `aggrPeriodDuration` value was not an ISO 8601 duration.
    #[error("Invalid aggrPeriodDuration value: '{0}'")]
    ParamAggrPeriodDurationInvalid(String),
    /// A `datasetId` entry was neither `@none` nor a URN.
    #[error("Invalid datasetId value: '{0}'")]
    ParamDatasetIdInvalid(String),
    /// `timerel=between` was provided without an `endTimeAt`.
    #[error("endTimeAt is required when timerel is 'between'")]
    ParamEndTimeAtRequired,
}
