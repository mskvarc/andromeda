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

use crate::{linked_retrieval_term::LinkedRetrievalTerm, query_narrowing::LocalScopeNarrowing, unsupported_representation::UnsupportedRepresentation};
use andromeda_http_headers::ngsild_response_format::NgsiLdResponseFormat;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{
    cursor::CursorDecodeError,
    error::QueryError,
    projection::exclusivity::ProjectionConflict,
    temporal::grouping::GroupingParseError,
};
use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentationError;
use thiserror::Error;

/// A query parameter the broker cannot act on.
#[derive(Debug, Error)]
pub enum ParameterError {
    /// `type=*` implies local scope (Table 5.2.33-1), so `local=false` contradicts it.
    #[error("type=* implies local scope and cannot be combined with local=false")]
    WildcardTypeWithLocalFalse,
    /// The parameter's value was rejected by the language that owns it.
    #[error(transparent)]
    Query(#[from] QueryError),

    /// The `options` parameter carried a value the operation does not support
    /// (clause 6.3.20).
    #[error("invalid options value: {0}")]
    OptionsInvalid(#[source] UnsupportedRepresentation),
    /// The `format` parameter value was not a recognised output format.
    #[error("invalid format value: {0}")]
    FormatInvalid(#[source] UnsupportedRepresentation),
    /// The `format` a merge selected is a rendering Table 6.5.3.4-1 does not
    /// admit as a merge input.
    #[error("invalid format value: {0}")]
    MergeFormatInvalid(#[source] MergeRepresentationError),
    /// The `NGSILD-Response-Format` header named a rendering the requested
    /// resource does not have.
    #[error("invalid NGSILD-Response-Format value: this resource has no '{0}' rendering")]
    ResponseFormatHeaderInvalid(NgsiLdResponseFormat),
    /// The `idPattern` parameter was present but empty.
    #[error("invalid idPattern: empty value")]
    IdPatternEmpty,
    /// `orderGeometry=GeometryCollection` is not supported.
    #[error("orderGeometry does not support GeometryCollection")]
    OrderGeometryCollection,
    /// `drop` and `keep` were both provided on an entity purge.
    #[error("drop and keep parameters are mutually exclusive")]
    DropKeepExclusive,
    /// `datasetId` and `deleteAll` were both provided on a purge.
    ///
    /// The two are contradictory: `deleteAll` removes every instance and
    /// `datasetId` names one, so `deleteAll` means "ignore datasetId" and the
    /// pair cannot both be honoured (clause 5.6.13). Reported as
    /// `BadRequestData`.
    #[error("datasetId and deleteAll parameters are mutually exclusive")]
    DatasetIdWithDeleteAll,
    /// The request combined clause 4.21 selectors that shape the same
    /// projection.
    #[error(transparent)]
    Projection(#[from] ProjectionConflict),
    /// `limit=0` appeared without the `count` option, which clause 6.3.10 Table
    /// 6.3.10-1 permits only when a count is requested.
    #[error("limit=0 is only allowed together with the count option")]
    LimitZeroWithoutCount,
    /// `cursor` and `offset` were both supplied (clause 5.5.9).
    ///
    /// An opaque `cursor` is a keyset continuation (clause 5.5.9.1) and `offset`
    /// its offset-based alternative (clause 5.5.9.2); the two page a collection
    /// by different mechanisms and cannot both be honoured, so the pair is a
    /// `BadRequestData`.
    #[error("cursor and offset parameters are mutually exclusive")]
    CursorWithOffset,
    /// The opaque pagination `cursor` could not be decoded (clause 5.5.9.1).
    ///
    /// The cursor is broker-emitted and followed unchanged, so any decode
    /// failure means the token was corrupted or forged; it is a `BadRequestData`.
    #[error("invalid pagination cursor: {0}")]
    CursorInvalid(#[source] CursorDecodeError),
    /// A `pick`/`omit` term nested a clause 4.21 sub-projection while the
    /// request asked for no join to retrieve the entity it describes.
    #[error("pick/omit select Linked Entity members but the request asks for no join to retrieve them")]
    ProjectionLinkedTermsWithoutJoin,
    /// A `pick`/`omit` term nested deeper than the requested traversal walks.
    #[error("pick/omit select Linked Entity members {depth} relationship(s) deep, beyond joinLevel={join_level}")]
    ProjectionDepthExceedsJoinLevel {
        /// How many Relationship hops the deepest nested term crosses.
        depth: u32,
        /// How many hops the requested traversal walks.
        join_level: u32,
    },
    /// A `q` filter crossed a Relationship while the request asked for no
    /// Linked Entity retrieval to reach it.
    #[error("q traverses a Linked Entity but the request asks for no join to retrieve them")]
    FilterTraversalWithoutJoin,
    /// A `q` filter chained more Relationship hops than the traversal walks.
    #[error("too deep query: q traverses {depth} Relationship(s), beyond joinLevel={join_level}")]
    FilterDepthExceedsJoinLevel {
        /// How many Relationship hops the deepest chain in the filter crosses.
        depth: u32,
        /// How many hops the requested traversal walks.
        join_level: u32,
    },
    /// A delete-by-query named a Linked Entity, which the operation cannot
    /// reach at all (clause 5.6.21.4 defines no `join` for it).
    #[error("delete by query must not name Linked Entities: {0}")]
    PurgeNamesLinkedEntities(LinkedRetrievalTerm),
    /// `join=inline` was combined with a temporal representation that has no
    /// per-instance nesting site.
    #[error("join=inline is not applicable to the {0} representation, which has no attribute instance to nest a linked entity under")]
    InlineJoinWithoutInstances(TemporalFormat),
    /// The deprecated `attrs` parameter named a structural Entity member.
    #[error("'{0}' is an Entity member and is not allowed in attrs")]
    AttrsReservedMember(AttributeName),
    /// A temporal query was issued without the required `timerel`/`timeAt` pair.
    #[error("timerel and timeAt are required for temporal query")]
    TimerelTimeatRequired,
    /// A selection was sent with none of the members that could narrow it.
    ///
    /// Clause 5.7.2 and 5.10.2 both require at least one selector, because a
    /// query with none of them asks for everything the broker holds.
    #[error("too wide query: at least one of {0} must be provided")]
    SelectorRequired(LocalScopeNarrowing),
    /// `format=aggregatedValues` was sent without the methods to aggregate by
    /// (clause 4.5.19).
    #[error("aggrMethods is required when format is 'aggregatedValues'")]
    AggrMethodsRequired,
    /// `aggrGroupBy` was sent without `format=groupedValues` (extension, study
    /// §4.9).
    ///
    /// The grouped response shape is a pure function of the format, so grouping
    /// only takes effect under the grouped format; asking to group any other
    /// representation is a contradiction rather than a silently ignored
    /// parameter.
    #[error("aggrGroupBy requires format=groupedValues (or options=groupedValues)")]
    AggrGroupByWithoutGroupedValues,
    /// `aggrGroupBy` did not parse as the extension grouping grammar (study §4.2).
    #[error("invalid aggrGroupBy: {0}")]
    AggrGroupByInvalid(#[source] GroupingParseError),
    /// An `aggrGroupBy` key named an attribute that is also an aggregation
    /// target (extension, study §4.9).
    ///
    /// Grouping by the same attribute the request aggregates is ambiguous — the
    /// value that keys the group is the value being summarised — so the study
    /// requires it to be spelled out rather than guessed.
    #[error("aggrGroupBy key '{0}' is also an aggregation target; group by a different member")]
    AggrGroupByOnAggregatedAttribute(AttributeName),
    /// `lastN` was combined with `aggrGroupBy` (extension, study OQ-5).
    ///
    /// `lastN` bounds the instances per attribute, which grouping then
    /// re-partitions; the two shape the result along different axes and the
    /// study leaves the combination out, as aggregation already leaves out
    /// `lastN`.
    #[error("lastN is not applicable together with aggrGroupBy")]
    LastNWithGrouping,
    /// A non-temporal query carried members that only temporal queries accept.
    #[error("entity queries must not contain temporalQ or aggrParams")]
    EntityQueryTemporalMembers,
    /// A temporal query omitted its required temporal window.
    #[error("temporal queries require temporalQ")]
    TemporalQueryMissing,
    /// Aggregation parameters were specified in both supported spec members.
    #[error("aggregation parameters must not be duplicated in temporalQ and aggrParams")]
    TemporalAggregationConflict,
    /// `orderBy` was sent on a query a registered Context Source takes part in
    /// (clauses 4.23.1, 5.7.2.4 and 5.7.4.4).
    ///
    /// Raised by the operation rather than by parameter validation: whether a
    /// registration takes part is what the Context Registry answers, not what
    /// the parameters say. The reading of "limited to the local scope" this
    /// rests on is recorded in `docs/entity-ordering-local-scope.md`.
    #[error(
        "orderBy is not applicable to a query a registered context source takes part in; limit the query to the local scope (local=true) or remove orderBy"
    )]
    OrderingWithParticipatingSource,
    /// An aggregated temporal representation named an attribute whose recorded
    /// values cannot be aggregated by one of the requested methods (clause
    /// 5.7.4.4, Table 4.5.19.1-1).
    #[error("attribute {attribute} is not eligible for the aggregation method {method}")]
    AggregationIneligible {
        /// The attribute the request named.
        attribute: AttributeName,
        /// The method the attribute's values cannot be aggregated by.
        method: AggrMethod,
    },
}
