//! The index path of a `~=` on an attribute's own value: the rows of the
//! attribute whose String target matches the pattern.
//!
//! The term compiles to one disjunction over the instance's
//! [`PatternTarget`]s, and its language-map arm has no index, so the
//! disjunction cannot be driven from one. The candidates read each target on
//! its own instead, as one branch of a `UNION ALL`: `value_string` through
//! `idx_ca_value_string_trgm`, `value_long_text` through
//! `idx_ca_value_long_text_trgm`, both keyed by the attribute and the
//! pattern's trigrams, and the `LanguageProperty` rows of the attribute
//! through the attribute key of `idx_ca_value_object`, each map unnested and
//! tested.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        instance_row::InstanceRow,
        keyed_branches::{branch, language_property_rows, union_of},
        pattern_term::{PatternTarget, pattern_on, pattern_targets},
    },
};
use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, SelectStatement, SimpleExpr};

/// `SELECT "entity_key"` over the rows of `attribute` whose own value matches
/// `pattern` (`~=`); `None` when the attribute was never recorded.
///
/// Only an attribute's own value is offered: a trailing member, a
/// sub-attribute or a hop reads a value no trigram index covers.
pub(crate) fn pattern_candidates(attribute: AttributeNameKey, pattern: &Pattern, rows: &EntityRowRefs) -> Option<SelectStatement> {
    let row = InstanceRow::Current;
    union_of(pattern_targets(row).map(|target| {
        let test = Condition::all().add(pattern_on(target, pattern, row));
        branch(rows, attribute, holds_target(target), test)
    }))
}

/// The rows that can hold `target`, stated so the planner reaches them
/// through an index when the target's own test gives it none.
///
/// A String column's pattern implies the column is set, which the partial
/// trigram index over it requires.
fn holds_target(target: PatternTarget) -> Option<SimpleExpr> {
    match target {
        PatternTarget::Column(_) => None,
        PatternTarget::LanguageMap => Some(language_property_rows()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        one_time_gate::CandidateBound,
        q_predicate::{keyed_term::keyed_candidates, lowered_filter::reading_of},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::SelectStatement;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("p27", 27), ("p03", 3), ("source", 7), ("p44", 44)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates(q: &str) -> Option<SelectStatement> {
        keyed_candidates(&reading_of(q), &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole)
            .into_iter()
            .next()
            .map(|keyed| keyed.select)
    }

    #[test]
    fn a_pattern_conjunct_on_an_own_value_is_index_served() {
        let statement = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        assert_statement_snapshot!("keyed-pattern-a-pattern-conjunct-on-an-own-value-is-index-served", statement);
    }

    /// The language branch names the rows `idx_ca_value_object` keys, so the
    /// planner reaches them by the attribute key, and tests every string of
    /// the map with no target language (clause 4.9).
    #[test]
    fn the_language_map_branch_reads_the_language_property_rows_of_the_attribute() {
        let statement = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        assert_statement_snapshot!(
            "keyed-pattern-the-language-map-branch-reads-the-language-property-rows-of-the-attribute",
            statement
        );
    }

    /// Clause 4.22: a past-due instance holds no value, so no branch offers it.
    #[test]
    fn every_branch_skips_an_expired_instance() {
        let statement = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        assert_statement_snapshot!("keyed-pattern-every-branch-skips-an-expired-instance", statement);
    }

    #[test]
    fn a_pattern_on_a_sub_attribute_or_a_value_member_is_not_index_served() {
        for q in [
            "\"https://example.com/p03\".\"https://example.com/source\"~=\"s.*\"",
            "\"https://example.com/p44\"[kind]~=\"k.*\"",
            "\"https://example.com/p44\"[kind.inner]~=\"k.*\"",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn a_pattern_on_an_unrecorded_attribute_is_not_index_served() {
        assert_eq!(candidates("\"https://example.com/unrecorded\"~=\"text-71.*\""), None);
    }

    #[test]
    fn a_not_pattern_term_is_not_index_served() {
        assert_eq!(candidates("\"https://example.com/p27\"!~=\"text-71.*\""), None);
    }
}
