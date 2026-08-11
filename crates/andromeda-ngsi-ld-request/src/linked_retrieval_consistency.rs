//! Whether a request's Linked Entity references agree with the traversal it
//! asked for (ETSI GS CIM 009 v1.9.1 clauses 5.7.2.4, 5.7.4.4 and 5.6.21.4).
//!
//! Two request members can reach across a Relationship into another entity: a
//! clause 4.21 sub-projection in `pick`/`omit`, which says how that entity
//! renders, and a clause 4.9 `rel{…}` hop in `q`, which asks a question about
//! it. Both are bounded by the same pair of parameters, `join` and `joinLevel`,
//! and clause 5.7.2.4 makes a reference that reaches beyond that bound a
//! `BadRequestData` rather than a silently ignored fragment — "too deep query"
//! for the filter. Clause 5.6.21.4 forbids the references outright on
//! delete-by-query, where nothing is rendered and no join can be asked for.
//!
//! Both members are bounded by the same test, because clause 5.7.2.4 states
//! both bullets with the identical phrase "use of Linked Entity retrieval is
//! not specified": Linked Entity retrieval is `join=flat` or `join=inline` and
//! nothing else. `@none` is the parameter's own default — clause 4.5.23 says
//! "By default, it will be '@none'" — so writing it can mean no more than
//! leaving the parameter out, and `joinLevel` is "only applicable if join
//! parameter is 'flat', or 'inline'", so an `@none` request declares no depth
//! either. A filter that hops therefore needs a retrieving join exactly as a
//! nested projection does. Filtering across a Relationship without pulling the
//! linked entities into the response is still available: clause 4.5.23.2 has an
//! `inline` response "only consist of Linking Entities", so `join=inline`
//! answers with the roots alone.
//!
//! On the temporal bindings this rule is part of the join extension rather than
//! a restatement of the spec's: clauses 5.7.3.4 and 5.7.4.4 admit no Linked
//! Entity retrieval at all there, raising an error when "projection attributes
//! or filter conditions indicate" one. Andromeda accepts the parameters and
//! bounds the references instead of refusing them, and the bound it applies is
//! this one.

use crate::{error::ParameterError, join_traversal::traversal_depth, linked_retrieval_term::LinkedRetrievalTerm};
use andromeda_ngsi_ld_query::{filter::expression::Expr, projection::grammar::RawProjectionTerm};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;

/// Reject a projection whose clause 4.21 nesting reaches past the join.
///
/// # Errors
///
/// Returns [`ParameterError::ProjectionLinkedTermsWithoutJoin`] when a nested
/// term is present and no retrieving join was requested, and
/// [`ParameterError::ProjectionDepthExceedsJoinLevel`] when the nesting is
/// deeper than the traversal walks.
pub fn validate_projection_join_depth(
    pick: Option<&[RawProjectionTerm]>,
    omit: Option<&[RawProjectionTerm]>,
    join: Option<JoinMode>,
    join_level: Option<NonZeroU32>,
) -> Result<(), ParameterError> {
    let depth = pick
        .unwrap_or(&[])
        .iter()
        .chain(omit.unwrap_or(&[]).iter())
        .map(RawProjectionTerm::linked_depth)
        .max()
        .unwrap_or(0);
    if depth == 0 {
        return Ok(());
    }

    // Clause 4.5.23: `@none` and an absent `join` both retrieve no linked
    // entity, so neither leaves a Linked Entity for the nesting to describe.
    match join {
        None | Some(JoinMode::None) => return Err(ParameterError::ProjectionLinkedTermsWithoutJoin),
        Some(JoinMode::Flat | JoinMode::Inline) => {}
    }

    let level = traversal_depth(join_level).get();
    if depth > level {
        return Err(ParameterError::ProjectionDepthExceedsJoinLevel { depth, join_level: level });
    }
    Ok(())
}

/// Reject a `q` filter whose clause 4.9 hops reach past the requested join.
///
/// Clause 5.7.2.4 bounds a filter's Linked Entity traversal by the request's
/// `join`/`joinLevel` pair: a hop where "use of Linked Entity retrieval is not
/// specified" has no traversal to run within, and a hop chain longer than
/// `joinLevel` reaches past the one it was given. On the temporal bindings the
/// same bound is applied as part of the join extension, in place of the
/// outright refusal clauses 5.7.3.4 and 5.7.4.4 call for.
///
/// # Errors
///
/// Returns [`ParameterError::FilterTraversalWithoutJoin`] when the filter hops
/// and no retrieving join was requested, and
/// [`ParameterError::FilterDepthExceedsJoinLevel`] when the hop chain is longer
/// than the traversal walks.
pub fn validate_filter_join_depth(q: Option<&Expr>, join: Option<JoinMode>, join_level: Option<NonZeroU32>) -> Result<(), ParameterError> {
    let depth = q.map_or(0, Expr::linked_depth);
    if depth == 0 {
        return Ok(());
    }
    // Clause 4.5.23: `@none` is the parameter's default, so stating it says no
    // more than omitting it, and `joinLevel` is applicable to neither. Neither
    // is Linked Entity retrieval, which is what the hop needs.
    match join {
        None | Some(JoinMode::None) => return Err(ParameterError::FilterTraversalWithoutJoin),
        Some(JoinMode::Flat | JoinMode::Inline) => {}
    }
    let level = traversal_depth(join_level).get();
    if depth > level {
        return Err(ParameterError::FilterDepthExceedsJoinLevel { depth, join_level: level });
    }
    Ok(())
}

/// Reject any Linked Entity reference on an operation that cannot carry a join.
///
/// Clause 5.6.21.4 gives delete-by-query no `join` parameter: it renders no
/// entity and retrieves none, so a `pick`/`omit` nesting has nothing to
/// describe and a `q` hop has no traversal budget it could ever be granted.
/// Both are rejected unconditionally rather than measured against a bound that
/// does not exist on this binding.
///
/// # Errors
///
/// Returns [`ParameterError::PurgeNamesLinkedEntities`] naming which member
/// crossed into a Linked Entity.
pub fn reject_linked_retrieval(pick: Option<&[RawProjectionTerm]>, omit: Option<&[RawProjectionTerm]>, q: Option<&Expr>) -> Result<(), ParameterError> {
    if q.is_some_and(Expr::contains_linked_hop) {
        return Err(ParameterError::PurgeNamesLinkedEntities(LinkedRetrievalTerm::Filter));
    }
    let nests = pick.unwrap_or(&[]).iter().chain(omit.unwrap_or(&[]).iter()).any(|term| term.linked_depth() > 0);
    if nests {
        return Err(ParameterError::PurgeNamesLinkedEntities(LinkedRetrievalTerm::Projection));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        linked_retrieval_consistency::{reject_linked_retrieval, validate_filter_join_depth, validate_projection_join_depth},
        linked_retrieval_term::LinkedRetrievalTerm,
    };
    use andromeda_ngsi_ld_query::{
        filter::expression::Expr,
        projection::grammar::{RawProjectionTerm, parse_projection_list},
    };
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn terms(raw: &str) -> Vec<RawProjectionTerm> {
        parse_projection_list(raw).expect("valid projection list")
    }

    fn filter(raw: &str) -> Expr {
        raw.parse().expect("valid filter")
    }

    fn level(value: u32) -> Option<NonZeroU32> {
        NonZeroU32::new(value)
    }

    /// A projection with no nesting says nothing about a linked entity, so it
    /// is valid with or without a join.
    #[test]
    fn a_flat_projection_is_accepted_without_a_join() {
        let pick = terms("speed,name");
        assert!(validate_projection_join_depth(Some(&pick), None, None, None).is_ok());
    }

    /// Clause 5.7.2.4: nested terms with no join name members of an entity the
    /// response will never carry.
    #[test]
    fn a_nested_pick_without_a_join_is_rejected() {
        let pick = terms("linkedTo{category}");
        let error = validate_projection_join_depth(Some(&pick), None, None, None).expect_err("must reject");
        assert!(matches!(error, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {error:?}");
    }

    /// Clause 4.5.23 makes `join=@none` the absence of a join, so it fails the
    /// rule exactly as an absent `join` does.
    #[test]
    fn a_nested_pick_with_the_none_join_mode_is_rejected() {
        let pick = terms("linkedTo{category}");
        let error = validate_projection_join_depth(Some(&pick), None, Some(JoinMode::None), None).expect_err("must reject");
        assert!(matches!(error, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {error:?}");
    }

    /// The rule reads `omit` as well as `pick` -- both carry clause 4.21
    /// sub-projections.
    #[test]
    fn a_nested_omit_without_a_join_is_rejected() {
        let omit = terms("linkedTo{name}");
        let error = validate_projection_join_depth(None, Some(&omit), None, None).expect_err("must reject");
        assert!(matches!(error, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {error:?}");
    }

    /// Clause 4.5.23: an absent `joinLevel` walks one hop, which is exactly
    /// what a single brace group needs.
    #[test]
    fn one_level_of_nesting_fits_the_default_join_level() {
        let pick = terms("linkedTo{category}");
        assert!(validate_projection_join_depth(Some(&pick), None, Some(JoinMode::Flat), None).is_ok());
    }

    /// Clause 5.7.2.4: two brace groups need two hops, and the one-hop default
    /// does not reach the inner one.
    #[test]
    fn two_levels_of_nesting_exceed_a_join_level_of_one() {
        let pick = terms("a{b{c}}");
        let error = validate_projection_join_depth(Some(&pick), None, Some(JoinMode::Flat), level(1)).expect_err("must reject");
        assert!(
            matches!(error, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {error:?}"
        );
    }

    /// The same projection is valid once the traversal is asked to walk as
    /// deep as the nesting reaches.
    #[test]
    fn two_levels_of_nesting_fit_a_join_level_of_two() {
        let pick = terms("a{b{c}}");
        assert!(validate_projection_join_depth(Some(&pick), None, Some(JoinMode::Flat), level(2)).is_ok());
    }

    /// The deepest term of the whole list decides, not the first: a shallow
    /// term alongside a deep one must not mask the violation.
    #[test]
    fn the_deepest_term_of_the_list_decides() {
        let pick = terms("flat,a{b{c}}");
        let error = validate_projection_join_depth(Some(&pick), None, Some(JoinMode::Inline), level(1)).expect_err("must reject");
        assert!(
            matches!(error, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, .. }),
            "got {error:?}"
        );
    }

    /// A filter that stays inside the entity under evaluation crosses no
    /// Relationship, so the traversal bound has nothing to say about it.
    #[test]
    fn a_local_filter_is_accepted_without_a_join() {
        assert!(validate_filter_join_depth(Some(&filter("speed>50")), None, None).is_ok());
        assert!(validate_filter_join_depth(None, None, None).is_ok());
    }

    /// Clause 5.7.2.4: a hop where "use of Linked Entity retrieval is not
    /// specified" asks the broker to walk into an entity the request never
    /// asked to reach. Clause 4.5.23 makes `@none` the parameter's default, so
    /// stating it is the same request as omitting it.
    #[test]
    fn a_traversing_filter_without_a_retrieving_join_is_rejected() {
        for join in [None, Some(JoinMode::None)] {
            let error = validate_filter_join_depth(Some(&filter("isParkedIn{category==\"commercial\"}")), join, None).expect_err("must reject");
            assert!(matches!(error, ParameterError::FilterTraversalWithoutJoin), "got {error:?} for join={join:?}");
        }
    }

    /// Clause 4.5.23: an absent `joinLevel` walks one hop, which is exactly
    /// what one brace group needs, under either retrieving mode. Clause
    /// 4.5.23.2 keeps `inline` from widening the response — it "shall only
    /// consist of Linking Entities" — so a filter that must not pull the linked
    /// entities into the answer still has a mode to run under.
    #[test]
    fn a_single_hop_fits_the_default_join_level_under_either_retrieving_mode() {
        for mode in [JoinMode::Flat, JoinMode::Inline] {
            assert!(
                validate_filter_join_depth(Some(&filter("isParkedIn{category}")), Some(mode), None).is_ok(),
                "single hop rejected under join={mode}"
            );
        }
    }

    /// Clause 5.7.2.4 "too deep query": two chained hops overspend the one-hop
    /// budget a `joinLevel` of one grants.
    #[test]
    fn two_filter_hops_exceed_a_join_level_of_one() {
        let error =
            validate_filter_join_depth(Some(&filter("isParkedIn{inDistrict{zone==\"north\"}}")), Some(JoinMode::Inline), level(1)).expect_err("must reject");
        assert!(
            matches!(error, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {error:?}"
        );
    }

    /// The same filter is valid once the traversal is asked to walk as deep as
    /// the hop chain reaches.
    #[test]
    fn two_filter_hops_fit_a_join_level_of_two() {
        assert!(validate_filter_join_depth(Some(&filter("isParkedIn{inDistrict{zone==\"north\"}}")), Some(JoinMode::Inline), level(2)).is_ok());
    }

    /// The deepest hop chain of the whole expression decides: a local predicate
    /// standing beside a chain must not mask it.
    #[test]
    fn the_deepest_hop_chain_of_the_expression_decides() {
        let error =
            validate_filter_join_depth(Some(&filter("speed>50;isParkedIn{inDistrict{zone}}")), Some(JoinMode::Flat), level(1)).expect_err("must reject");
        assert!(
            matches!(error, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {error:?}"
        );
    }

    /// Clause 5.6.21.4: a delete-by-query that stays local is untouched by the
    /// rule.
    #[test]
    fn a_purge_naming_no_linked_entity_is_accepted() {
        let pick = terms("speed,name");
        assert!(reject_linked_retrieval(Some(&pick), None, Some(&filter("speed>50"))).is_ok());
    }

    /// Clause 5.6.21.4: no `join` exists on delete-by-query, so a traversing
    /// filter is rejected however the rest of the request is written.
    #[test]
    fn a_purge_whose_filter_traverses_is_rejected() {
        let error = reject_linked_retrieval(None, None, Some(&filter("isParkedIn{category}"))).expect_err("must reject");
        assert!(
            matches!(error, ParameterError::PurgeNamesLinkedEntities(LinkedRetrievalTerm::Filter)),
            "got {error:?}"
        );
    }

    /// Clause 5.6.21.4: the same holds for a nested projection term, in `pick`
    /// as in `omit`.
    #[test]
    fn a_purge_whose_projection_nests_is_rejected() {
        for (pick, omit) in [(Some(terms("linkedTo{category}")), None), (None, Some(terms("linkedTo{name}")))] {
            let error = reject_linked_retrieval(pick.as_deref(), omit.as_deref(), None).expect_err("must reject");
            assert!(
                matches!(error, ParameterError::PurgeNamesLinkedEntities(LinkedRetrievalTerm::Projection)),
                "got {error:?}"
            );
        }
    }
}
