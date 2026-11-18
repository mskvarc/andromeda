use crate::{
    error::ParameterError,
    query::entity::{
        admitted::AdmittedEntityQuery,
        operation::EntityQueryOperation,
        parameters::EntityQueryParameters,
        purge::{PurgeEntitiesParameters, PurgeSelection},
    },
};
use andromeda_linked_retrieval::admission::{LinkedMember, ReachingMember};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    filter::{compilation::FilterAdmission, complexity::FilterComplexityLimits, filter_error::FilterError, language_targeting::LanguageTargeting},
    query_error::QueryError,
};
use std::num::NonZeroUsize;

/// The broker's default complexity limits (1000 comparisons, 16 levels) at the
/// given `q_target_languages` level.
fn broker(language_targeting: LanguageTargeting) -> FilterAdmission {
    FilterAdmission {
        complexity: FilterComplexityLimits::new(
            NonZeroUsize::new(1000).expect("a non-zero limit"),
            NonZeroUsize::new(16).expect("a non-zero limit"),
        ),
        language_targeting,
    }
}

fn admitted(operation: EntityQueryOperation, query: &str, admission: FilterAdmission) -> Result<AdmittedEntityQuery, ParameterError> {
    serde_urlencoded::from_str::<EntityQueryParameters>(query)
        .expect("a decodable query")
        .admit(operation, admission)
}

fn purged(query: &str, admission: FilterAdmission) -> Result<PurgeSelection, ParameterError> {
    serde_urlencoded::from_str::<PurgeEntitiesParameters>(query)
        .expect("a decodable purge")
        .admit(admission)
}

const BOTH_OPERATIONS: [EntityQueryOperation; 2] = [EntityQueryOperation::Query, EntityQueryOperation::EntityMap];

fn too_many_comparisons(count: usize) -> String {
    let values = (0..count).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
    format!("type=Vehicle&q=speed%3D%3D{values}")
}

/// Clause 5.5.6: a filter of 1 001 comparisons is too complex under the
/// default limits and one of 1 000 is not, for Query Entities, the Entity Map it
/// creates and Purge Entities alike.
#[test]
fn a_q_past_the_comparison_limit_is_refused_at_validation() {
    let admission = broker(LanguageTargeting::Specification);
    for operation in BOTH_OPERATIONS {
        admitted(operation, &too_many_comparisons(1000), admission).expect("1000 comparisons are within the limit");
        let refused = admitted(operation, &too_many_comparisons(1001), admission).expect_err("1001 comparisons are too many");
        assert!(
            matches!(refused, ParameterError::Query(QueryError::Filter(FilterError::TooManyComparisons { .. }))),
            "{operation:?}: {refused:?}"
        );
    }
    purged(&too_many_comparisons(1000), admission).expect("1000 comparisons are within the limit");
    let refused = purged(&too_many_comparisons(1001), admission).expect_err("1001 comparisons are too many");
    assert!(
        matches!(refused, ParameterError::Query(QueryError::Filter(FilterError::TooManyComparisons { .. }))),
        "{refused:?}"
    );
}

/// Clause 5.5.6: a filter nested deeper than the limit is `TooComplexQuery`.
#[test]
fn a_q_nested_past_the_depth_limit_is_refused_at_validation() {
    let nested = |levels: usize| (1..levels).fold("a>1;b>1".to_owned(), |inner, level| format!("c{level}>1|({inner})"));
    let query = |levels: usize| format!("type=Vehicle&q={}", nested(levels).replace('>', "%3E").replace(';', "%3B").replace('|', "%7C"));
    let admission = broker(LanguageTargeting::Specification);
    admitted(EntityQueryOperation::Query, &query(16), admission).expect("16 levels are within the limit");
    let refused = admitted(EntityQueryOperation::Query, &query(17), admission).expect_err("17 levels are too deep");
    assert!(
        matches!(refused, ParameterError::Query(QueryError::Filter(FilterError::NestedTooDeep { .. }))),
        "{refused:?}"
    );
}

/// Clause 4.9: a full language tag as a target language is admitted at
/// the language-tags level and refused at the specification level.
#[test]
fn a_language_tag_target_is_admitted_only_at_the_language_tags_level() {
    let query = "type=Vehicle&q=name%5Ben-GB%5D%3D%3D%22x%22";
    for operation in BOTH_OPERATIONS {
        admitted(operation, query, broker(LanguageTargeting::LanguageTags)).expect("the language-tags level admits a tag");
        let refused = admitted(operation, query, broker(LanguageTargeting::Specification)).expect_err("the specification level refuses a tag");
        assert!(
            matches!(refused, ParameterError::Query(QueryError::Filter(FilterError::LanguageTagNotAdmitted(_)))),
            "{operation:?}: {refused:?}"
        );
    }
    purged(query, broker(LanguageTargeting::LanguageTags)).expect("the language-tags level admits a tag");
    let refused = purged(query, broker(LanguageTargeting::Specification)).expect_err("the specification level refuses a tag");
    assert!(
        matches!(refused, ParameterError::Query(QueryError::Filter(FilterError::LanguageTagNotAdmitted(_)))),
        "{refused:?}"
    );
}

/// Clauses 5.7.2.4 and 5.14.4.4: a hop without a retrieving `join`
/// reaches no Linked Entity.
#[test]
fn a_hop_without_a_join_is_refused_by_query_entities_and_entity_map_creation() {
    for operation in BOTH_OPERATIONS {
        for query in ["type=Car&q=isParkedIn%7Bcategory%7D", "type=Car&join=%40none&q=isParkedIn%7Bcategory%7D"] {
            let refused = admitted(operation, query, broker(LanguageTargeting::Specification)).expect_err(query);
            assert!(
                matches!(refused, ParameterError::FilterTraversalWithoutJoin),
                "{operation:?} {query}: {refused:?}"
            );
        }
    }
}

/// Clauses 5.7.2.4 and 5.14.4.4: a hop deeper than `joinLevel` is a too deep query.
#[test]
fn a_hop_deeper_than_the_join_level_is_refused() {
    for operation in BOTH_OPERATIONS {
        let refused = admitted(
            operation,
            "type=Car&join=flat&joinLevel=1&q=isParkedIn%7BinDistrict%7Bzone%7D%7D",
            broker(LanguageTargeting::Specification),
        )
        .expect_err("two hops past one level");
        assert!(
            matches!(refused, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "{operation:?}: {refused:?}"
        );
        admitted(
            operation,
            "type=Car&join=flat&joinLevel=2&q=isParkedIn%7BinDistrict%7Bzone%7D%7D",
            broker(LanguageTargeting::Specification),
        )
        .expect("two levels reach two hops");
    }
}

/// Clause 5.6.21.4: Purge Entities takes no `join`, so it refuses every hop.
#[test]
fn a_purge_refuses_every_hop() {
    let refused = purged("type=Car&q=isParkedIn%7Bcategory%7D", broker(LanguageTargeting::Specification)).expect_err("a purge retrieves nothing");
    assert!(
        matches!(
            refused,
            ParameterError::LinkedRetrievalUnavailable {
                member: LinkedMember::Reaching(ReachingMember::Filter),
                ..
            }
        ),
        "{refused:?}"
    );
}

/// The admissions run complexity, then hops, then target languages, so a
/// filter breaking two rules reports the first.
#[test]
fn the_admissions_run_complexity_then_hops_then_target_languages() {
    let admission = broker(LanguageTargeting::Specification);
    let hop_and_tag = admitted(EntityQueryOperation::Query, "type=Car&q=r%7Bname%5Ben-GB%5D%7D%3D%3D%22x%22", admission).expect_err("a hop and a tag");
    assert!(matches!(hop_and_tag, ParameterError::FilterTraversalWithoutJoin), "{hop_and_tag:?}");
    let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
    let hop_and_size = admitted(EntityQueryOperation::Query, &format!("type=Car&q=r%7Ba%7D%3D%3D{values}"), admission).expect_err("a hop and a size");
    assert!(
        matches!(hop_and_size, ParameterError::Query(QueryError::Filter(FilterError::TooManyComparisons { .. }))),
        "{hop_and_size:?}"
    );
}

/// A filter that is no refused one carries its admission, and a query
/// carrying none has none.
#[test]
fn an_admitted_query_carries_the_admission_of_its_q() {
    let admission = broker(LanguageTargeting::Specification);
    let with = admitted(EntityQueryOperation::Query, "type=Vehicle&q=speed%3E5", admission).expect("admitted");
    assert!(with.q.is_some() && with.filter().is_some());
    let without = admitted(EntityQueryOperation::Query, "type=Vehicle", admission).expect("admitted");
    assert!(without.q.is_none() && without.filter().is_none());
}

/// An Entity Map narrows the identifiers and selectors of an admitted query
/// without touching its `q` or the admission of it.
#[test]
fn narrowing_the_selection_leaves_the_q_and_its_admission() {
    let mut query = admitted(
        EntityQueryOperation::Query,
        "type=Vehicle&q=speed%3E5",
        broker(LanguageTargeting::Specification),
    )
    .expect("admitted");
    let held: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier");
    *query.selection_mut().id = Some(vec![held.clone()]);
    pretty_assertions::assert_eq!(query.id, Some(vec![held]));
    assert!(query.q.is_some() && query.filter().is_some());
}
