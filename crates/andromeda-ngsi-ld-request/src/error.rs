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

use crate::linked_retrieval_term::LinkedRetrievalTerm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat};
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
    /// `pick` and `omit` were both provided on a query.
    #[error("pick and omit are mutually exclusive (clause 4.21)")]
    PickOmitExclusive,
    /// The legacy `attrs` selector was combined with the clause-4.21
    /// projection selectors. They are alternative ways to shape a response.
    #[error("pick/omit and attrs are mutually exclusive")]
    PickOmitWithAttrs,
    /// `limit=0` appeared without the `count` option, which clause 6.3.10 Table
    /// 6.3.10-1 permits only when a count is requested.
    #[error("limit=0 is only allowed together with the count option (clause 6.3.10)")]
    LimitZeroWithoutCount,
    /// A `pick`/`omit` term nested a clause 4.21 sub-projection while the
    /// request asked for no join to retrieve the entity it describes.
    #[error("pick/omit select Linked Entity members but the request asks for no join to retrieve them (clause 5.7.2.4)")]
    ProjectionLinkedTermsWithoutJoin,
    /// A `pick`/`omit` term nested deeper than the requested traversal walks.
    #[error("pick/omit select Linked Entity members {depth} relationship(s) deep, beyond joinLevel={join_level} (clause 5.7.2.4)")]
    ProjectionDepthExceedsJoinLevel {
        /// How many Relationship hops the deepest nested term crosses.
        depth: u32,
        /// How many hops the requested traversal walks.
        join_level: u32,
    },
    /// A `q` filter crossed a Relationship while the request asked for no
    /// Linked Entity retrieval to reach it.
    #[error("q traverses a Linked Entity but the request asks for no join to retrieve them (clause 5.7.2.4)")]
    FilterTraversalWithoutJoin,
    /// A `q` filter chained more Relationship hops than the traversal walks.
    #[error("too deep query: q traverses {depth} Relationship(s), beyond joinLevel={join_level} (clause 5.7.2.4)")]
    FilterDepthExceedsJoinLevel {
        /// How many Relationship hops the deepest chain in the filter crosses.
        depth: u32,
        /// How many hops the requested traversal walks.
        join_level: u32,
    },
    /// A delete-by-query named a Linked Entity, which the operation cannot
    /// reach at all (clause 5.6.21.4 defines no `join` for it).
    #[error("delete by query must not name Linked Entities: {0} (clause 5.6.21.4)")]
    PurgeNamesLinkedEntities(LinkedRetrievalTerm),
    /// `join=inline` was combined with a temporal representation that has no
    /// per-instance nesting site.
    #[error(
        "join=inline is not applicable to the {0} representation, which has no attribute instance to nest a linked entity under (clauses 4.5.5, 4.5.19, 4.5.23)"
    )]
    InlineJoinWithoutInstances(TemporalFormat),
    /// The deprecated `attrs` parameter named a structural Entity member.
    #[error("'{0}' is an Entity member and is not allowed in attrs")]
    AttrsReservedMember(AttributeName),
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
    /// A POST query body named a resource type other than `Query`.
    ///
    /// Clause 5.2.23 fixes the body's `type` as `Query`; a different value is a
    /// malformed request rather than one the broker reinterprets.
    #[error("the query body type must be 'Query', not '{0}'")]
    RequestTypeInvalid(String),
    /// A non-temporal query carried members that only temporal queries accept.
    #[error("entity queries must not contain temporalQ or aggrParams")]
    EntityQueryTemporalMembers,
    /// A temporal query omitted its required temporal window.
    #[error("temporal queries require temporalQ")]
    TemporalQueryMissing,
    /// Aggregation parameters were specified in both supported spec members.
    #[error("aggregation parameters must not be duplicated in temporalQ and aggrParams")]
    TemporalAggregationConflict,
}
