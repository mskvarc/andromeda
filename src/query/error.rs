//! Typed error type for the NGSI-LD query pipeline.
//!
//! Every fallible step in parsing, compiling, and building SQL for a query
//! emits a `QueryError`. Variants are strongly-typed: source-bearing causes
//! (`regex::Error`, `serde_json::Error`, `langtag::InvalidLangTag`,
//! `std::num::ParseFloatError`) are chained through `#[source]` / `#[from]`
//! so `.source()` walks the full cause chain.
//!
//! Mapping onto the ETSI CIM 009 clause 5.6.3 error types lives in
//! `crate::core::error` -- structurally malformed inputs become
//! `MalformedRequest`, semantically invalid data becomes `QueryValidation`.

use crate::query::compiled::IcuCollationError;
use andromeda_domain::geo_rel::GeoRelParseError;

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

    // -----------------------------------------------------------------------
    // idPattern
    // -----------------------------------------------------------------------
    /// The `idPattern` parameter was not a valid regular expression.
    #[error("idPattern: invalid regex")]
    IdPatternRegex(#[from] regex::Error),

    // -----------------------------------------------------------------------
    // Geo query (NGSI-LD 4.10)
    // -----------------------------------------------------------------------
    /// The `georel` value could not be parsed.
    #[error("geo: {0}")]
    GeoRel(#[from] GeoRelParseError),
    /// Some but not all of the four required geo components were provided.
    #[error("geo: all of georel, geometry, coordinates must be provided together")]
    GeoIncomplete,

    // -----------------------------------------------------------------------
    // ICU collation (NGSI-LD 4.23)
    // -----------------------------------------------------------------------
    /// The `orderBy` collation identifier was invalid.
    #[error(transparent)]
    IcuCollation(#[from] IcuCollationError),

    // -----------------------------------------------------------------------
    // HTTP parameter preflight (clause 5.6.3)
    // -----------------------------------------------------------------------
    /// An `id` value could not be parsed as a URI.
    #[error("Invalid id value: '{0}'")]
    ParamIdInvalid(String),
    /// An empty value appeared in a comma-separated id list.
    #[error("Invalid id: empty value in comma-separated list")]
    ParamIdListEmpty,
    /// The `idPattern` parameter was present but empty.
    #[error("Invalid idPattern: empty value")]
    ParamIdPatternEmpty,
    /// An unrecognised token appeared in the `options` parameter list.
    #[error("Invalid options value: '{0}'")]
    ParamOptionsInvalid(String),
    /// The `format` parameter value was not a recognised output format.
    #[error("Invalid format value: '{0}'")]
    ParamFormatInvalid(String),
    /// The `orderGeometry` parameter value was not a valid geometry kind.
    #[error("Invalid orderGeometry value: '{0}'")]
    ParamOrderGeometryInvalid(String),
    /// `orderGeometry=GeometryCollection` is not supported.
    #[error("orderGeometry does not support GeometryCollection")]
    ParamOrderGeometryCollection,
    /// The `orderFrom` JSON value could not be parsed.
    #[error("Invalid orderFrom value: '{value}' is not valid JSON")]
    ParamOrderFromInvalid {
        /// The raw value from the request.
        value: String,
        /// The underlying JSON parse error.
        #[source]
        source: serde_json::Error,
    },
    /// `drop` and `keep` were both provided on an entity purge.
    #[error("drop and keep parameters are mutually exclusive")]
    ParamDropKeepExclusive,
    /// The `timerel` value was not one of `before`, `after`, `between`.
    #[error("Invalid timerel value: {0}")]
    ParamTimerelInvalid(String),
    /// The `aggrMethod` value was not a recognised aggregation method.
    #[error("Invalid aggrMethod value: {0}")]
    ParamAggrMethodInvalid(String),
    /// A temporal query was issued without the required `timerel`/`timeAt` pair.
    #[error("timerel and timeAt are required for temporal query")]
    ParamTimerelTimeatRequired,
    /// `timerel` and `timeAt` must both be present or both absent.
    #[error("timerel and timeAt must both be present or both absent")]
    ParamTimerelTimeatPair,
    /// `timerel=between` was provided without an `endTimeAt`.
    #[error("endTimeAt is required when timerel is 'between'")]
    ParamEndTimeAtRequired,
    /// A parameter failed validation for a reason not covered above.
    #[error("{0}")]
    ParamOther(String),
}
