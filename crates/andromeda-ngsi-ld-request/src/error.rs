//! Failures of the NGSI-LD query-parameter model (clause 6.3.x).
//!
//! These are faults of a *request*, not of a query language: a `format` value
//! that names no rendering, `drop` and `keep` sent together, a selection with
//! no selector at all. They stay out of `QueryError` so the specification's
//! query-language crate declares no variant it cannot construct.
//!
//! The language's own failures arrive through [`QueryError`], which this wraps:
//! a parameter is parsed by the language wherever the language defines its
//! grammar, and there is one error chain either way.

use crate::{pagination::page_size::PageSizeExceeded, query_narrowing::LocalScopeNarrowing, unsupported_representation::UnsupportedRepresentation};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_linked_retrieval::admission::{AdmissionError, LinkedMember, LinkedOperation, ReachingMember};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{error::QueryError, projection::exclusivity::ProjectionConflict, temporal::grouping::GroupingParseError};
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
    /// `geometryProperty` was sent with a response media type other than
    /// `application/geo+json` (clauses 5.7.1.4, 5.7.2.4 and 5.14.4.4).
    #[error("geometryProperty is only applicable when the Accept header is application/geo+json, not {0}")]
    GeometryPropertyWithoutGeoJson(ResponseMediaType),
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
    /// `limit` asked for more elements than one page of the collection holds,
    /// a `TooManyResults` (clause 5.5.6).
    #[error(transparent)]
    PageSizeExceeded(#[from] PageSizeExceeded),
    /// `cursor` and `offset` were both supplied (clause 5.5.9).
    ///
    /// An opaque `cursor` is a keyset continuation (clause 5.5.9.1) and `offset`
    /// its offset-based alternative (clause 5.5.9.2); the two page a collection
    /// by different mechanisms and cannot both be honoured, so the pair is a
    /// `BadRequestData`.
    #[error("cursor and offset parameters are mutually exclusive")]
    CursorWithOffset,
    /// A `cursor` reached a collection that offers no cursor walk: the broker
    /// runs with offset pagination only, or the binding carries its query in a
    /// body no pointer can dereference (clauses 5.5.9.1 and 6.3.10).
    ///
    /// The parameter is one the operation does not offer, an `InvalidRequest`
    /// (clause 6.3.20).
    #[error("cursor pagination is not available on this request; page by limit and offset")]
    CursorUnavailable,
    /// A `cursor` was combined with a clause 4.23 `orderBy`.
    ///
    /// A cursor is a position in identifier order (clause 5.5.9.1) and cannot
    /// walk a requested ordering, which pages by offset; dropping either
    /// parameter silently would answer a different request than the one sent,
    /// so the pair is a `BadRequestData`.
    #[error("cursor cannot be combined with orderBy; page an ordered query by limit and offset")]
    CursorWithOrdering,
    /// A followed page pointer was requested under another `@context` than the
    /// walk it belongs to: clause 5.5.9.1 keeps one `@context` throughout a
    /// pagination, and an attempt to use a different one is a `BadRequestData`.
    #[error("the page pointer belongs to a walk under another @context; follow it with the @context the walk started with")]
    PageContextChanged,
    /// A pagination parameter reached an operation that pages nothing: Purge
    /// Entities defines no `limit`, `offset` or `cursor` (Table 6.4.3.3-1), and
    /// clause 6.3.20 answers a parameter the operation does not define with
    /// `InvalidRequest`.
    #[error("limit, offset and cursor are not defined for this operation")]
    PaginationUndefined,
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
    /// A request referenced a Linked Entity on an operation that retrieves
    /// none: Purge Entities (clause 5.6.21.4), and the temporal operations
    /// without the `temporal_linked_retrieval` extension (clauses 5.7.3.4,
    /// 5.7.4.4 and 5.14.5.4).
    #[error("{operation} does not retrieve Linked Entities, but {member}")]
    LinkedRetrievalUnavailable {
        /// The operation the request is.
        operation: LinkedOperation,
        /// The member that referenced a Linked Entity.
        member: LinkedMember,
    },
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
    /// `aggrMethods` or `aggrPeriodDuration` was sent while neither `format`
    /// nor `options` selected `aggregatedValues`, the only representation
    /// Tables 6.18.3.2-1 and 6.19.3.1-1 apply them to (clause 6.3.20).
    #[error("aggrMethods and aggrPeriodDuration are only applicable if aggregatedValues is present in the format or options parameter")]
    AggregationWithoutAggregatedValues,
    /// `joinLevel` was sent while `join` asked for no Linked Entity retrieval:
    /// it is "only applicable if join parameter is 'flat' or 'inline'"
    /// (Table 6.5.3.1-1, clause 6.3.20).
    #[error("joinLevel is only applicable if the join parameter is 'flat' or 'inline'")]
    JoinLevelWithoutJoin,
    /// `containedBy` was sent without the `joinLevel` it is "only applicable"
    /// beside (Tables 6.4.3.2-1 and 6.5.3.1-1, clause 6.3.20).
    #[error("containedBy is only applicable if joinLevel is present")]
    ContainedByWithoutJoinLevel,
    /// `entityMapLifetime` was sent without `entityMap=true`, the map it
    /// would set the lifetime of (Tables 6.4.3.2-1, 6.5.3.1-1 and 6.18.3.2-1,
    /// clause 6.3.20).
    #[error("entityMapLifetime is only applicable if entityMap is set to true")]
    EntityMapLifetimeWithoutEntityMap,
    /// `aggrGroupBy` was sent without `format=groupedValues` (grouping
    /// extension).
    ///
    /// The grouped response shape is a pure function of the format, so grouping
    /// only takes effect under the grouped format; asking to group any other
    /// representation is a contradiction rather than a silently ignored
    /// parameter.
    #[error("aggrGroupBy requires format=groupedValues (or options=groupedValues)")]
    AggrGroupByWithoutGroupedValues,
    /// `aggrGroupBy` did not parse as the extension grouping grammar.
    #[error("invalid aggrGroupBy: {0}")]
    AggrGroupByInvalid(#[source] GroupingParseError),
    /// An `aggrGroupBy` key named an attribute that is also an aggregation
    /// target (grouping extension).
    ///
    /// Grouping by the same attribute the request aggregates is ambiguous: the
    /// value that keys the group is the value being summarised. The extension
    /// requires it to be spelled out rather than guessed.
    #[error("aggrGroupBy key '{0}' is also an aggregation target; group by a different member")]
    AggrGroupByOnAggregatedAttribute(AttributeName),
    /// `lastN` was combined with `aggrGroupBy` (grouping extension).
    ///
    /// `lastN` bounds the instances per attribute, which grouping then
    /// re-partitions; the two shape the result along different axes, so the
    /// extension excludes the combination, as aggregation already excludes
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
    /// the parameters say.
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

// Hand-written because each admission failure keeps the parameter error, and
// so the problem type and wording, the client receives for it; no
// derive maps one enum's variants onto another's.
impl From<AdmissionError> for ParameterError {
    fn from(error: AdmissionError) -> ParameterError {
        match error {
            AdmissionError::NotRetrievable { operation, member } => ParameterError::LinkedRetrievalUnavailable { operation, member },
            AdmissionError::WithoutJoin(ReachingMember::Projection) => ParameterError::ProjectionLinkedTermsWithoutJoin,
            AdmissionError::WithoutJoin(ReachingMember::Filter) => ParameterError::FilterTraversalWithoutJoin,
            AdmissionError::DeeperThanJoinLevel {
                member: ReachingMember::Projection,
                depth,
                join_level,
            } => ParameterError::ProjectionDepthExceedsJoinLevel {
                depth: depth.get(),
                join_level: join_level.get(),
            },
            AdmissionError::DeeperThanJoinLevel {
                member: ReachingMember::Filter,
                depth,
                join_level,
            } => ParameterError::FilterDepthExceedsJoinLevel {
                depth: depth.get(),
                join_level: join_level.get(),
            },
            AdmissionError::JoinLevelWithoutJoin => ParameterError::JoinLevelWithoutJoin,
            AdmissionError::ContainedByWithoutJoinLevel => ParameterError::ContainedByWithoutJoinLevel,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::ParameterError;
    use andromeda_linked_retrieval::admission::{AdmissionError, LinkedMember, LinkedOperation, ReachingMember, TemporalJoin};
    use std::num::NonZeroU32;

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("a positive depth")
    }

    /// An operation that retrieves no Linked Entity names itself and the
    /// member that referenced one.
    #[test]
    fn a_refused_reference_keeps_its_operation_and_member() {
        let operation = LinkedOperation::QueryTemporal(TemporalJoin::Spec);
        let error = ParameterError::from(AdmissionError::NotRetrievable {
            operation,
            member: LinkedMember::Join,
        });
        assert!(
            matches!(error, ParameterError::LinkedRetrievalUnavailable { operation: o, member: LinkedMember::Join } if o == operation),
            "got {error:?}"
        );
    }

    /// A nesting without a join keeps its projection error.
    #[test]
    fn a_nesting_without_a_join_keeps_its_projection_error() {
        let error = ParameterError::from(AdmissionError::WithoutJoin(ReachingMember::Projection));
        assert!(matches!(error, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {error:?}");
    }

    /// A hop without a join keeps its filter error.
    #[test]
    fn a_hop_without_a_join_keeps_its_filter_error() {
        let error = ParameterError::from(AdmissionError::WithoutJoin(ReachingMember::Filter));
        assert!(matches!(error, ParameterError::FilterTraversalWithoutJoin), "got {error:?}");
    }

    /// A nesting past `joinLevel` keeps its depth and level.
    #[test]
    fn a_nesting_too_deep_keeps_its_depth_and_level() {
        let error = ParameterError::from(AdmissionError::DeeperThanJoinLevel {
            member: ReachingMember::Projection,
            depth: depth(2),
            join_level: depth(1),
        });
        assert!(
            matches!(error, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {error:?}"
        );
    }

    /// A hop chain past `joinLevel` is the clause 5.7.2.4 "too deep query".
    #[test]
    fn a_hop_chain_too_deep_is_a_too_deep_query() {
        let error = ParameterError::from(AdmissionError::DeeperThanJoinLevel {
            member: ReachingMember::Filter,
            depth: depth(3),
            join_level: depth(2),
        });
        assert!(
            matches!(error, ParameterError::FilterDepthExceedsJoinLevel { depth: 3, join_level: 2 }),
            "got {error:?}"
        );
        assert!(error.to_string().starts_with("too deep query"));
    }

    #[test]
    fn a_join_level_without_a_join_keeps_its_own_error() {
        let error = ParameterError::from(AdmissionError::JoinLevelWithoutJoin);
        assert!(matches!(error, ParameterError::JoinLevelWithoutJoin), "got {error:?}");
    }

    #[test]
    fn a_contained_by_without_a_join_level_keeps_its_own_error() {
        let error = ParameterError::from(AdmissionError::ContainedByWithoutJoinLevel);
        assert!(matches!(error, ParameterError::ContainedByWithoutJoinLevel), "got {error:?}");
    }
}
