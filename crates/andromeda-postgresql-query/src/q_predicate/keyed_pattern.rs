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
//! tested. The branches together keep exactly the rows the disjunction keeps,
//! and a bounded read stops at the first branches that fill the bound.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        instance_row::InstanceRow,
        local_target::{LocalTarget, local_target},
        pattern_term::{PatternTarget, pattern_on, pattern_targets},
        projection_probe::{Ca, of_attribute, projection_column, standing},
    },
};
use andromeda_ngsi_ld_query::filter::{
    path::QueryPath,
    predicate::{PatternPolarity, Predicate},
};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Alias, ExprTrait, Query, SelectStatement, SimpleExpr, UnionType};

/// The name the union of the per-target branches is read under.
const MATCHES: &str = "matches";

/// `SELECT "entity_key"` over the rows of the attribute whose own value
/// matches `pred`, `None` unless `pred` is a `~=` on a recorded attribute's
/// own value.
///
/// A path with a trailing member, a sub-attribute or a hop reads a value no
/// trigram index covers.
pub(crate) fn pattern_candidates(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    let Predicate::MatchPattern {
        path: QueryPath::Local(path),
        pattern,
    } = pred
    else {
        return None;
    };
    match local_target(path) {
        LocalTarget::OwnValue => {}
        LocalTarget::TrailingMember(_) | LocalTarget::NestedMember | LocalTarget::SubAttribute(_) => return None,
    }
    let attribute = names.attributes.get(&path.attr)?;
    let row = InstanceRow::Current;
    let mut branches = pattern_targets(row).map(|target| {
        let mut branch = Query::select();
        branch
            .expr(projection_column(CurrentAttributes::EntityKey))
            .from(rows.table(CurrentAttributes::Table).alias(Ca))
            .and_where(of_attribute(attribute))
            .and_where(standing());
        if let Some(rows_holding) = holds_target(target) {
            branch.and_where(rows_holding);
        }
        branch.and_where(pattern_on(target, pred, pattern, PatternPolarity::Match, row));
        branch.take()
    });
    let mut union = branches.next()?;
    for branch in branches {
        union.union(UnionType::All, branch);
    }
    let mut candidates = Query::select();
    candidates.column(CurrentAttributes::EntityKey).from_subquery(union, Alias::new(MATCHES));
    Some(candidates.take())
}

/// The rows that can hold `target`, stated so the planner reaches them
/// through an index when the target's own test gives it none.
///
/// A String column's pattern implies the column is set, which the partial
/// trigram index over it requires. A `languageMap` is a `LanguageProperty`
/// row's `value_object`, which `idx_ca_value_object` keys by attribute.
fn holds_target(target: PatternTarget) -> Option<SimpleExpr> {
    match target {
        PatternTarget::Column(_) => None,
        PatternTarget::LanguageMap => Some(
            projection_column(CurrentAttributes::AttributeKind)
                .eq(int_literal(i64::from(AttributeKind::LanguageProperty.code())))
                .and(projection_column(CurrentAttributes::ValueObject).is_not_null()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_pattern::pattern_candidates,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("p27", 27), ("p03", 3), ("source", 7), ("p44", 44)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
        }
    }

    fn candidates(q: &str) -> Option<String> {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        pattern_candidates(&pred, &EntityRowRefs::unaliased(None), &names()).map(|select| select.to_string(PostgresQueryBuilder))
    }

    #[test]
    fn a_pattern_conjunct_on_an_own_value_is_index_served() {
        let sql = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        assert!(
            sql.starts_with(r#"SELECT "entity_key" FROM (SELECT "ca"."entity_key" FROM "current_attributes" AS "ca""#),
            "{sql}"
        );
        assert_eq!(sql.matches(r#""ca"."attribute_name_key" = 27"#).count(), 3, "one branch per target: {sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
        assert!(sql.contains(r#""ca"."value_string" ~ 'text-71.*'"#), "{sql}");
        assert!(sql.contains(r#""ca"."value_long_text" ~ 'text-71.*'"#), "{sql}");
        assert!(sql.ends_with(r#") AS "matches""#), "{sql}");
    }

    /// The language branch names the rows `idx_ca_value_object` keys, so the
    /// planner reaches them by the attribute key, and tests every string of
    /// the map with no target language (clause 4.9).
    #[test]
    fn the_language_map_branch_reads_the_language_property_rows_of_the_attribute() {
        let sql = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        let language = sql.rsplit("UNION ALL").next().expect("the last branch");
        assert!(
            language.contains(r#"("ca"."attribute_kind" = (5) AND "ca"."value_object" IS NOT NULL)"#),
            "{language}"
        );
        assert!(
            language.contains(r#"jsonb_array_elements((CASE WHEN ("ca"."attribute_kind" = (5)) THEN "ca"."value_object" END)) AS "ls""#),
            "{language}"
        );
        assert!(language.contains(r#"("ls"."value" ->> ('@value')) ~ 'text-71.*'"#), "{language}");
        assert!(!language.contains("@language"), "{language}");
    }

    /// Clause 4.22: a past-due instance holds no value, so no branch offers it.
    #[test]
    fn every_branch_skips_an_expired_instance() {
        let sql = candidates("\"https://example.com/p27\"~=\"text-71.*\"").expect("keyed");
        assert_eq!(sql.matches(r#""ca"."expires_at" IS NULL"#).count(), 3, "{sql}");
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
