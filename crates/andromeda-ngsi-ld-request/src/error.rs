//! Failures of the NGSI-LD query-parameter model (clause 6.3.x).
//!
//! These are faults of a *request*, not of a query language: a `format` value
//! that names no rendering, `drop` and `keep` sent together, a selection with
//! no selector at all. They used to sit in `QueryError`, which meant the
//! specification's query-language crate declared a dozen variants it could not
//! construct, and only the binary ever did.
//!
//! The language's own failures arrive through [`QueryError`], which this wraps:
//! a parameter is parsed by the language wherever the language defines its
//! grammar, and there is one error chain either way.

use andromeda_ngsi_ld_query::error::QueryError;
use thiserror::Error;

/// A query parameter the broker cannot act on.
#[derive(Debug, Error)]
pub enum ParameterError {
    /// The parameter's value was rejected by the language that owns it.
    #[error(transparent)]
    Query(#[from] QueryError),

    /// An unrecognised token appeared in the `options` parameter list.
    #[error("Invalid options value: '{0}'")]
    OptionsInvalid(String),
    /// The `format` parameter value was not a recognised output format.
    #[error("Invalid format value: '{0}'")]
    FormatInvalid(String),
    /// The `idPattern` parameter was present but empty.
    #[error("Invalid idPattern: empty value")]
    IdPatternEmpty,
    /// An `orderBy` term carried a direction outside the clause 4.23 grammar.
    #[error("Invalid orderBy direction: '{0}'")]
    OrderByDirectionInvalid(String),
    /// The `orderGeometry` parameter value was not a valid geometry kind.
    #[error("Invalid orderGeometry value: '{0}'")]
    OrderGeometryInvalid(String),
    /// `orderGeometry=GeometryCollection` is not supported.
    #[error("orderGeometry does not support GeometryCollection")]
    OrderGeometryCollection,
    /// The `orderFrom` JSON value could not be parsed.
    #[error("Invalid orderFrom value: '{value}' is not valid JSON")]
    OrderFromInvalid {
        /// The raw value from the request.
        value: String,
        /// The underlying JSON parse error.
        #[source]
        source: serde_json::Error,
    },
    /// `drop` and `keep` were both provided on an entity purge.
    #[error("drop and keep parameters are mutually exclusive")]
    DropKeepExclusive,
    /// The `timerel` value was not one of `before`, `after`, `between`.
    #[error("Invalid timerel value: {0}")]
    TimerelInvalid(String),
    /// The `timeproperty` value did not name a temporal property (clause 4.8).
    #[error("Invalid timeproperty value: {0}")]
    TimePropertyInvalid(String),
    /// The `aggrMethod` value was not a recognised aggregation method.
    #[error("Invalid aggrMethod value: {0}")]
    AggrMethodInvalid(String),
    /// A temporal query was issued without the required `timerel`/`timeAt` pair.
    #[error("timerel and timeAt are required for temporal query")]
    TimerelTimeatRequired,
    /// A selection was sent with none of the members that could narrow it.
    ///
    /// Clause 5.7.2 and 5.10.2 both require at least one selector, because a
    /// query with none of them asks for everything the broker holds.
    #[error("{0}")]
    SelectorRequired(&'static str),
    /// `format=aggregatedValues` was sent without the methods to aggregate by
    /// (clause 4.5.19).
    #[error("aggrMethods is required when format is 'aggregatedValues'")]
    AggrMethodsRequired,
}
