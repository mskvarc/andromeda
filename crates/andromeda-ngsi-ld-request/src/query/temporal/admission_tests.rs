use crate::{
    error::ParameterError,
    query::{
        filter_not_admitted::FilterNotAdmitted,
        temporal::{
            TemporalQueryParameters,
            admitted::AdmittedTemporalRequest,
            admitted_delete::AdmittedTemporalDelete,
            delete_parameters::TemporalDeleteParameters,
            operation::TemporalQueryOperation,
        },
    },
};
use andromeda_linked_retrieval::admission::{LinkedMember, LinkedOperation, ReachingMember, TemporalJoin};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{compilation::FilterAdmission, complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    temporal::window::TimeInterval,
};
use std::num::NonZeroUsize;

const WINDOW: &str = "timerel=after&timeAt=2024-01-01T00:00:00Z";

fn broker(language_targeting: LanguageTargeting) -> FilterAdmission {
    FilterAdmission {
        complexity: FilterComplexityLimits::new(
            NonZeroUsize::new(1000).expect("a non-zero limit"),
            NonZeroUsize::new(16).expect("a non-zero limit"),
        ),
        language_targeting,
    }
}

fn admitted(
    operation: TemporalQueryOperation,
    linked_retrieval: TemporalJoin,
    query: &str,
    admission: FilterAdmission,
) -> Result<AdmittedTemporalRequest, ParameterError> {
    serde_urlencoded::from_str::<TemporalQueryParameters>(query)
        .expect("a decodable query")
        .admit(operation, linked_retrieval, admission)
}

fn deleted(query: &str, admission: FilterAdmission) -> Result<AdmittedTemporalDelete, ParameterError> {
    serde_urlencoded::from_str::<TemporalDeleteParameters>(query)
        .expect("a decodable delete")
        .admit(admission)
}

const ALL_OPERATIONS: [TemporalQueryOperation; 3] = [
    TemporalQueryOperation::Retrieve,
    TemporalQueryOperation::Query,
    TemporalQueryOperation::EntityMap,
];

fn many_comparisons(count: usize) -> String {
    let values = (0..count).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
    format!("type=Vehicle&{WINDOW}&q=speed%3D%3D{values}")
}

/// CP-3, clause 5.5.6: a filter of 1 001 comparisons is too complex under the
/// default limits and one of 1 000 is not, for every temporal operation and the
/// delete-by-query.
#[test]
fn a_q_past_the_comparison_limit_is_refused_at_validation() {
    let admission = broker(LanguageTargeting::Specification);
    for operation in ALL_OPERATIONS {
        admitted(operation, TemporalJoin::Spec, &many_comparisons(1000), admission).expect("1000 comparisons are within the limit");
        let refused = admitted(operation, TemporalJoin::Spec, &many_comparisons(1001), admission).expect_err("1001 comparisons are too many");
        assert!(
            matches!(refused, ParameterError::Query(QueryError::QTooManyComparisons { .. })),
            "{operation:?}: {refused:?}"
        );
    }
    deleted(&many_comparisons(1000), admission).expect("1000 comparisons are within the limit");
    let refused = deleted(&many_comparisons(1001), admission).expect_err("1001 comparisons are too many");
    assert!(matches!(refused, ParameterError::Query(QueryError::QTooManyComparisons { .. })), "{refused:?}");
}

/// CP-4, clause 4.9: a full language tag as a target language is admitted at
/// the language-tags level and refused at the specification level.
#[test]
fn a_language_tag_target_is_admitted_only_at_the_language_tags_level() {
    let query = format!("type=Vehicle&{WINDOW}&q=name%5Ben-GB%5D%3D%3D%22x%22");
    for operation in ALL_OPERATIONS {
        admitted(operation, TemporalJoin::Spec, &query, broker(LanguageTargeting::LanguageTags)).expect("the language-tags level admits a tag");
        let refused =
            admitted(operation, TemporalJoin::Spec, &query, broker(LanguageTargeting::Specification)).expect_err("the specification level refuses a tag");
        assert!(
            matches!(refused, ParameterError::Query(QueryError::LanguageTagNotAdmitted(_))),
            "{operation:?}: {refused:?}"
        );
    }
    deleted(&query, broker(LanguageTargeting::LanguageTags)).expect("the language-tags level admits a tag");
    let refused = deleted(&query, broker(LanguageTargeting::Specification)).expect_err("the specification level refuses a tag");
    assert!(matches!(refused, ParameterError::Query(QueryError::LanguageTagNotAdmitted(_))), "{refused:?}");
}

/// CP-7, clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4: a temporal operation retrieves
/// no Linked Entity, so a hop is refused, and with the temporal Linked Entity
/// extension it follows the Core rule.
#[test]
fn a_temporal_operation_refuses_a_hop_unless_the_extension_retrieves_it() {
    let hop = format!("type=Vehicle&{WINDOW}&q=isParkedIn%7Bcategory%7D");
    let joined = format!("type=Vehicle&{WINDOW}&join=flat&joinLevel=1&q=isParkedIn%7Bcategory%7D");
    for operation in ALL_OPERATIONS {
        let refused =
            admitted(operation, TemporalJoin::Spec, &hop, broker(LanguageTargeting::Specification)).expect_err("a temporal read retrieves no Linked Entity");
        assert!(
            matches!(
                refused,
                ParameterError::LinkedRetrievalUnavailable {
                    operation: refused_operation,
                    member: LinkedMember::Reaching(ReachingMember::Filter),
                } if refused_operation == operation.linked(TemporalJoin::Spec)
            ),
            "{operation:?}: {refused:?}"
        );
        admitted(operation, TemporalJoin::Extended, &joined, broker(LanguageTargeting::Specification)).expect("the extension retrieves a hop within its join");
    }
}

/// The extension follows the Core rule for a hop: it needs a retrieving join,
/// and no deeper than `joinLevel` (CP-5 and CP-6).
#[test]
fn under_the_extension_a_hop_needs_a_join_that_reaches_it() {
    let admission = broker(LanguageTargeting::Specification);
    let without_join = format!("type=Vehicle&{WINDOW}&q=isParkedIn%7Bcategory%7D");
    let refused = admitted(TemporalQueryOperation::Query, TemporalJoin::Extended, &without_join, admission).expect_err("no join");
    assert!(matches!(refused, ParameterError::FilterTraversalWithoutJoin), "{refused:?}");
    let too_deep = format!("type=Vehicle&{WINDOW}&join=flat&joinLevel=1&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
    let refused = admitted(TemporalQueryOperation::Query, TemporalJoin::Extended, &too_deep, admission).expect_err("too deep");
    assert!(
        matches!(refused, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
        "{refused:?}"
    );
}

/// The temporal delete-by-query takes no `join`, so it refuses a hop as Purge
/// Entities does (clause 5.6.21.4), and a filter without one passes.
#[test]
fn the_delete_by_query_refuses_a_hop() {
    let admission = broker(LanguageTargeting::Specification);
    let refused = deleted(&format!("type=Vehicle&{WINDOW}&q=isParkedIn%7Bcategory%7D"), admission).expect_err("a hop");
    assert!(
        matches!(
            refused,
            ParameterError::LinkedRetrievalUnavailable {
                operation: LinkedOperation::TemporalDeleteByQuery,
                member: LinkedMember::Reaching(ReachingMember::Filter),
            }
        ),
        "{refused:?}"
    );
    let delete = deleted(&format!("type=Vehicle&{WINDOW}&q=speed%3E5"), admission).expect("a filter without a hop is admitted");
    assert!(delete.selection().filter().is_some() && delete.selection().local == Some(true));
}

/// The admissions run complexity, then hops, then target languages.
#[test]
fn the_admissions_run_complexity_then_hops_then_target_languages() {
    let admission = broker(LanguageTargeting::Specification);
    let hop_and_tag = format!("type=Vehicle&{WINDOW}&q=r%7Bname%5Ben-GB%5D%7D%3D%3D%22x%22");
    let refused = admitted(TemporalQueryOperation::Query, TemporalJoin::Spec, &hop_and_tag, admission).expect_err("a hop and a tag");
    assert!(matches!(refused, ParameterError::LinkedRetrievalUnavailable { .. }), "{refused:?}");
    let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
    let hop_and_size = format!("type=Vehicle&{WINDOW}&q=r%7Ba%7D%3D%3D{values}");
    let refused = admitted(TemporalQueryOperation::Query, TemporalJoin::Spec, &hop_and_size, admission).expect_err("a hop and a size");
    assert!(matches!(refused, ParameterError::Query(QueryError::QTooManyComparisons { .. })), "{refused:?}");
}

/// A derived query that names no `q` is admitted as it is, and one that names
/// a `q` is refused, since no validation covered it.
#[test]
fn a_derived_query_is_admitted_unfiltered_only_without_a_q() {
    let bare = serde_urlencoded::from_str::<TemporalQueryParameters>(WINDOW).expect("a decodable query");
    assert!(AdmittedTemporalRequest::unfiltered(bare).is_ok_and(|query| query.filter().is_none()));
    let filtered = serde_urlencoded::from_str::<TemporalQueryParameters>(&format!("{WINDOW}&q=speed%3E5")).expect("a decodable query");
    assert!(matches!(AdmittedTemporalRequest::unfiltered(filtered), Err(FilterNotAdmitted)));
}

/// Reading an admitted query over another window, or of the entities it
/// matched, keeps its `q` and the admission of it.
#[test]
fn narrowing_the_window_and_the_selection_keeps_the_q_and_its_admission() {
    let mut query = admitted(
        TemporalQueryOperation::Query,
        TemporalJoin::Spec,
        &format!("type=Vehicle&{WINDOW}&q=speed%3E5"),
        broker(LanguageTargeting::Specification),
    )
    .expect("admitted");
    let from = "2024-01-01T00:00:00Z".parse().expect("a valid instant");
    let to = "2024-02-01T00:00:00Z".parse().expect("a valid instant");
    query.set_interval(TimeInterval::Between { from, to });
    let held: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier");
    query.select_only(vec![held.clone()]);
    *query.selection_mut().entity_selectors = Vec::new();
    pretty_assertions::assert_eq!(query.id, Some(vec![held]));
    pretty_assertions::assert_eq!(query.end_time_at, Some(to));
    assert!(query.q.is_some() && query.filter().is_some());
}
