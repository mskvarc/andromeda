use crate::admission::{AdmissionError, LinkedMember, LinkedOperation, LinkedRequest, ReachingMember, TemporalJoin, admit_request};
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    projection::grammar::{ProjectionList, RawProjectionTerm},
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use pretty_assertions::assert_eq;
use std::num::NonZeroU32;

/// One admission case: a projection and a filter written as request text, the
/// join parameters, and what the operation answers.
struct Case {
    /// The `pick` text, if any.
    pick: Option<&'static str>,
    /// The `q` text, if any.
    q: Option<&'static str>,
    /// The `join` value.
    join: Option<JoinMode>,
    /// The `joinLevel` value.
    join_level: Option<u32>,
}

impl Case {
    const fn pick(pick: &'static str, join: Option<JoinMode>, join_level: Option<u32>) -> Case {
        Case {
            pick: Some(pick),
            q: None,
            join,
            join_level,
        }
    }

    const fn filter(q: &'static str, join: Option<JoinMode>, join_level: Option<u32>) -> Case {
        Case {
            pick: None,
            q: Some(q),
            join,
            join_level,
        }
    }

    fn admit(&self, operation: LinkedOperation) -> Result<(), AdmissionError> {
        let pick: Option<Vec<RawProjectionTerm>> = self.pick.map(|raw| Vec::from(raw.parse::<ProjectionList>().expect("valid projection list")));
        let q: Option<Expr> = self.q.map(|raw| raw.parse().expect("valid filter"));
        let request = LinkedRequest::builder()
            .pick(pick.as_deref())
            .q(q.as_ref())
            .join(self.join)
            .join_level(self.join_level.and_then(NonZeroU32::new))
            .build();
        admit_request(operation, &request)
    }
}

fn depth(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("a positive depth")
}

const PROJECTION: ReachingMember = ReachingMember::Projection;
const FILTER: ReachingMember = ReachingMember::Filter;

/// The clause 5.7.1.4 cases and what Retrieve Entity answers to each.
fn retrieve_cases() -> Vec<(Case, Result<(), AdmissionError>)> {
    vec![
        (Case::pick("a", None, None), Ok(())),
        (Case::pick("a{b}", None, None), Err(AdmissionError::WithoutJoin(PROJECTION))),
        (Case::pick("a{b}", Some(JoinMode::None), None), Err(AdmissionError::WithoutJoin(PROJECTION))),
        (Case::pick("a{b}", Some(JoinMode::Flat), None), Ok(())),
        (
            Case::pick("a{b{c}}", Some(JoinMode::Inline), Some(1)),
            Err(AdmissionError::DeeperThanJoinLevel {
                member: PROJECTION,
                depth: depth(2),
                join_level: depth(1),
            }),
        ),
        (Case::pick("a{b{c}}", Some(JoinMode::Flat), Some(2)), Ok(())),
    ]
}

/// The clause 5.7.2.4 cases beyond the Retrieve Entity ones.
fn query_cases() -> Vec<(Case, Result<(), AdmissionError>)> {
    let mut cases = retrieve_cases();
    cases.extend([
        (
            Case::filter("isParkedIn{category}==\"x\"", None, None),
            Err(AdmissionError::WithoutJoin(FILTER)),
        ),
        (
            Case::filter("isParkedIn{category}==\"x\"", Some(JoinMode::None), None),
            Err(AdmissionError::WithoutJoin(FILTER)),
        ),
        (Case::filter("isParkedIn{category}", Some(JoinMode::Flat), None), Ok(())),
        (Case::filter("isParkedIn{category}", Some(JoinMode::Inline), None), Ok(())),
        (
            Case::filter("isParkedIn{inDistrict{zone}}==\"n\"", Some(JoinMode::Flat), Some(1)),
            Err(AdmissionError::DeeperThanJoinLevel {
                member: FILTER,
                depth: depth(2),
                join_level: depth(1),
            }),
        ),
        (
            Case::filter("speed>50;isParkedIn{inDistrict{zone}}", Some(JoinMode::Flat), Some(1)),
            Err(AdmissionError::DeeperThanJoinLevel {
                member: FILTER,
                depth: depth(2),
                join_level: depth(1),
            }),
        ),
    ]);
    cases
}

fn assert_cases(operation: LinkedOperation, cases: &[(Case, Result<(), AdmissionError>)]) {
    for (index, (case, expected)) in cases.iter().enumerate() {
        assert_eq!(&case.admit(operation), expected, "case {index} of {operation:?}");
    }
}

/// Clause 5.7.1.4: a nesting is bounded by `join` and `joinLevel`.
#[test]
fn retrieve_entity_bounds_a_nesting_by_the_join() {
    assert_cases(LinkedOperation::RetrieveEntity, &retrieve_cases());
}

/// Clauses 5.7.2.4 and 5.14.4.4: nestings and hops are bounded alike, for the
/// query and for the Entity Map it creates.
#[test]
fn query_entities_and_the_entity_map_query_bound_nestings_and_hops() {
    assert_cases(LinkedOperation::QueryEntities, &query_cases());
    assert_cases(LinkedOperation::EntityMapQuery, &query_cases());
}

/// Clause 5.6.21.4: plain terms and local filters are untouched.
#[test]
fn purge_admits_flat_terms_and_plain_filters() {
    assert_eq!(Case::pick("a", None, None).admit(LinkedOperation::PurgeEntities), Ok(()));
    assert_eq!(Case::filter("speed>50", None, None).admit(LinkedOperation::PurgeEntities), Ok(()));
}

/// Clause 5.6.21.4: a nesting is refused whatever join is asked for.
#[test]
fn purge_refuses_any_nesting() {
    assert_eq!(
        Case::pick("a{b}", Some(JoinMode::Flat), Some(5)).admit(LinkedOperation::PurgeEntities),
        Err(AdmissionError::NotRetrievable {
            operation: LinkedOperation::PurgeEntities,
            member: LinkedMember::Reaching(PROJECTION),
        })
    );
}

/// Clause 5.6.21.4: so is a hop.
#[test]
fn purge_refuses_any_hop() {
    assert_eq!(
        Case::filter("isParkedIn{category}", Some(JoinMode::Flat), Some(5)).admit(LinkedOperation::PurgeEntities),
        Err(AdmissionError::NotRetrievable {
            operation: LinkedOperation::PurgeEntities,
            member: LinkedMember::Reaching(FILTER),
        })
    );
}

/// Clause 5.7.3.4: no nesting on the temporal retrieval, whatever the join.
#[test]
fn spec_temporal_retrieve_refuses_a_nesting_whatever_the_join() {
    let operation = LinkedOperation::RetrieveTemporal(TemporalJoin::Spec);
    for join in [None, Some(JoinMode::None), Some(JoinMode::Flat), Some(JoinMode::Inline)] {
        let refused = Case::pick("a{b}", join, None).admit(operation);
        let expected_member = match join {
            Some(JoinMode::Flat | JoinMode::Inline) => LinkedMember::Join,
            Some(JoinMode::None) | None => LinkedMember::Reaching(PROJECTION),
        };
        assert_eq!(
            refused,
            Err(AdmissionError::NotRetrievable {
                operation,
                member: expected_member,
            }),
            "join {join:?}"
        );
    }
}

/// Clauses 5.7.4.4 and 5.14.5.4 refuse a nesting, a hop and a retrieving
/// `join`; `@none` asks for nothing and passes.
fn assert_spec_temporal_query(operation: LinkedOperation) {
    assert_eq!(
        Case::pick("a{b}", None, None).admit(operation),
        Err(AdmissionError::NotRetrievable {
            operation,
            member: LinkedMember::Reaching(PROJECTION),
        })
    );
    assert_eq!(
        Case::filter("isParkedIn{category}", None, None).admit(operation),
        Err(AdmissionError::NotRetrievable {
            operation,
            member: LinkedMember::Reaching(FILTER),
        })
    );
    let join_request = Case {
        pick: None,
        q: None,
        join: Some(JoinMode::Flat),
        join_level: None,
    };
    assert_eq!(
        join_request.admit(operation),
        Err(AdmissionError::NotRetrievable {
            operation,
            member: LinkedMember::Join,
        })
    );
    let none_join = Case {
        pick: None,
        q: None,
        join: Some(JoinMode::None),
        join_level: None,
    };
    assert_eq!(none_join.admit(operation), Ok(()));
    assert_eq!(Case::pick("a", None, None).admit(operation), Ok(()));
    assert_eq!(Case::filter("speed>1", None, None).admit(operation), Ok(()));
}

#[test]
fn spec_temporal_query_refuses_a_nesting_a_hop_and_a_retrieving_join() {
    assert_spec_temporal_query(LinkedOperation::QueryTemporal(TemporalJoin::Spec));
}

#[test]
fn spec_temporal_entity_map_follows_the_temporal_query() {
    assert_spec_temporal_query(LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Spec));
}

/// With the extension, the temporal operations answer as their Core
/// counterparts do.
#[test]
fn extended_temporal_operations_follow_the_core_operations() {
    assert_cases(LinkedOperation::RetrieveTemporal(TemporalJoin::Extended), &retrieve_cases());
    assert_cases(LinkedOperation::QueryTemporal(TemporalJoin::Extended), &query_cases());
    assert_cases(LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Extended), &query_cases());
}

/// Clause 5.8.1.4: a Subscription passing the clause 5.2.12 checks is added,
/// and none of those checks bounds a nesting or a hop.
#[test]
fn subscriptions_admit_every_reference() {
    let operation = LinkedOperation::Subscription;
    assert_eq!(Case::pick("a{b}", None, None).admit(operation), Ok(()));
    assert_eq!(Case::pick("a{b{c}}", Some(JoinMode::Flat), Some(1)).admit(operation), Ok(()));
    assert_eq!(Case::filter("isParkedIn{category}", None, None).admit(operation), Ok(()));
}

/// A request carrying only the join parameters.
const fn join_only(join: Option<JoinMode>, join_level: Option<u32>) -> Case {
    Case {
        pick: None,
        q: None,
        join,
        join_level,
    }
}

/// Every operation but a Subscription, with and without the extension.
const JOIN_LEVEL_BOUND_OPERATIONS: [LinkedOperation; 10] = [
    LinkedOperation::RetrieveEntity,
    LinkedOperation::QueryEntities,
    LinkedOperation::EntityMapQuery,
    LinkedOperation::PurgeEntities,
    LinkedOperation::RetrieveTemporal(TemporalJoin::Spec),
    LinkedOperation::RetrieveTemporal(TemporalJoin::Extended),
    LinkedOperation::QueryTemporal(TemporalJoin::Spec),
    LinkedOperation::QueryTemporal(TemporalJoin::Extended),
    LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Spec),
    LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Extended),
];

/// Table 6.5.3.1-1: `joinLevel` is "only applicable if join parameter is
/// 'flat' or 'inline'", and `@none` asks for no traversal (clause 4.5.23).
#[test]
fn a_join_level_without_a_retrieving_join_is_refused_outside_subscriptions() {
    for operation in JOIN_LEVEL_BOUND_OPERATIONS {
        for join in [None, Some(JoinMode::None)] {
            assert_eq!(
                join_only(join, Some(2)).admit(operation),
                Err(AdmissionError::JoinLevelWithoutJoin),
                "{operation:?} join {join:?}"
            );
        }
    }
}

/// The Core operations and their extended temporal counterparts take a
/// `joinLevel` beside a retrieving join.
#[test]
fn a_join_level_beside_a_retrieving_join_is_admitted() {
    for operation in [
        LinkedOperation::RetrieveEntity,
        LinkedOperation::QueryEntities,
        LinkedOperation::EntityMapQuery,
        LinkedOperation::QueryTemporal(TemporalJoin::Extended),
    ] {
        for join in [JoinMode::Flat, JoinMode::Inline] {
            assert_eq!(join_only(Some(join), Some(2)).admit(operation), Ok(()), "{operation:?} join {join:?}");
        }
    }
}

/// Clause 5.8.1.4 checks no applicability, so a Subscription keeps a
/// `joinLevel` whatever its join.
#[test]
fn a_subscription_keeps_a_join_level_without_a_retrieving_join() {
    for join in [None, Some(JoinMode::None)] {
        assert_eq!(join_only(join, Some(2)).admit(LinkedOperation::Subscription), Ok(()), "join {join:?}");
    }
}

/// The refusal reads as one sentence naming the operation and the member.
#[test]
fn a_refusal_names_the_operation_and_the_member() {
    let refused = Case::filter("isParkedIn{category}", None, None)
        .admit(LinkedOperation::PurgeEntities)
        .expect_err("refused");
    assert_eq!(
        refused.to_string(),
        "Purge Entities does not retrieve Linked Entities, but q traverses a Linked Entity"
    );
}
