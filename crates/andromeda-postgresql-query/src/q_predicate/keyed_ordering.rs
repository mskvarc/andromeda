//! The index path of an ordering (`>`, `>=`, `<`, `<=`) or a range (`==a..b`)
//! on an attribute's own value whose operand is a `DateTime`, a `Date` or a
//! `Time`.
//!
//! Such a term compiles to its typed column and, where an operand may match a
//! `languageMap` string, a language arm ([`OwnValueArms`]) that has no index,
//! so the candidates read each arm on its own: the typed arm as a range scan
//! of `idx_ca_value_date_time` or, for a `Date` and a `Time`, which are held
//! as fixed-width text under their value kinds, of `idx_ca_value_string`; any
//! language arm through the attribute's `LanguageProperty` rows.
//!
//! No other operand is offered. A number or a boolean compares against its
//! typed column alone, which the planner already drives from its index. A
//! String orders under the ICU root collation (clause 4.9 recommends the UCA),
//! which the byte-ordered `idx_ca_value_string` cannot range over.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::range_arms,
        instance_row::InstanceRow,
        keyed_branches::{branch, language_property_rows, union_of},
        local_target::{LocalTarget, local_target},
        ordering_term::ordering_arms,
        own_value_arms::OwnValueArms,
    },
};
use andromeda_ngsi_ld_query::filter::{
    path::QueryPath,
    predicate::{Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
};
use sea_query::{Condition, SelectStatement};

/// Whether a value index orders an operand the way the term compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandOrder {
    /// The typed column's index orders the operand's kind as the term does.
    Indexed,
    /// No index orders the operand's kind as the term does, or the planner
    /// already drives the term from one.
    Unindexed,
}

/// The order a value index gives `scalar`.
const fn operand_order(scalar: &Scalar) -> OperandOrder {
    match scalar {
        Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => OperandOrder::Indexed,
        Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) => OperandOrder::Unindexed,
    }
}

/// `SELECT "entity_key"` over the rows of the attribute whose own value
/// satisfies `pred`, `None` unless `pred` is an ordering or a range with
/// indexed operands on a recorded attribute's own value.
///
/// A range outside its bounds (`!=a..b`) never narrows: an Entity holds when
/// its value lies outside, which an index of values inside cannot enumerate.
pub(crate) fn ordering_candidates(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    let row = InstanceRow::Current;
    let (path, arms) = match pred {
        Predicate::Ordering {
            path: QueryPath::Local(path),
            op,
            rhs,
        } => match operand_order(rhs) {
            OperandOrder::Indexed => (path, ordering_arms(pred, *op, rhs, row)),
            OperandOrder::Unindexed => return None,
        },
        Predicate::Equality {
            path: QueryPath::Local(path),
            membership: SetMembership::Within,
            rhs: CompareValue::Range { min, max },
        } => match (operand_order(min), operand_order(max)) {
            (OperandOrder::Indexed, OperandOrder::Indexed) => (path, range_arms(min, max, row)),
            (OperandOrder::Indexed | OperandOrder::Unindexed, OperandOrder::Unindexed) | (OperandOrder::Unindexed, OperandOrder::Indexed) => {
                return None;
            }
        },
        Predicate::Ordering {
            path: QueryPath::Linked { .. },
            ..
        }
        | Predicate::Equality { .. }
        | Predicate::Exists(_)
        | Predicate::MatchPattern { .. }
        | Predicate::NotMatchPattern { .. } => return None,
    };
    if !path.sub_attrs.is_empty() {
        return None;
    }
    match local_target(path) {
        LocalTarget::OwnValue => {}
        LocalTarget::TrailingMember { .. } | LocalTarget::TargetLanguage(_) | LocalTarget::NestedMember(_) => return None,
    }
    let attribute = names.attributes.get(&path.attr)?;
    let OwnValueArms { typed, language } = arms;
    let typed = branch(rows, attribute, None, typed);
    let language = language.map(|language| branch(rows, attribute, Some(language_property_rows()), Condition::all().add(language)));
    union_of([typed].into_iter().chain(language))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_ordering::ordering_candidates,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("observedOn", 21), ("source", 7), ("reading", 30)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates(q: &str) -> Option<String> {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        ordering_candidates(&pred, &EntityRowRefs::unaliased(None), &names()).map(|select| select.to_string(PostgresQueryBuilder))
    }

    /// A `languageMap` holds no time value, so the one branch ranges over the
    /// date-time index.
    #[test]
    fn a_date_time_ordering_reads_the_date_time_index_alone() {
        let sql = candidates("\"https://example.com/observedOn\">2024-01-15T12:00:00Z").expect("keyed");
        assert!(sql.contains(r#""ca"."attribute_name_key" = 21"#), "{sql}");
        assert!(sql.contains(r#""ca"."value_date_time" > '2024-01-15 12:00:00.000000 +00:00'"#), "{sql}");
        assert!(!sql.contains("UNION ALL"), "{sql}");
        assert!(!sql.contains("value_object"), "{sql}");
    }

    /// A `Date` is held as its fixed-width text under value kind 8, whose
    /// byte order is chronological, so its branch ranges over the string index.
    #[test]
    fn a_date_ordering_reads_the_string_index_by_its_text() {
        let sql = candidates("\"https://example.com/observedOn\"<=2024-01-15").expect("keyed");
        assert!(sql.contains(r#""ca"."value_string" <= '2024-01-15'"#), "{sql}");
        assert!(sql.contains(r#"nullif("ca"."value_kind" = (8), FALSE)"#), "{sql}");
        assert!(!sql.contains("COLLATE"), "{sql}");
    }

    /// A `Time` is held in its fixed-width storage encoding under value kind 9.
    #[test]
    fn a_time_ordering_reads_the_string_index_by_its_fixed_width_encoding() {
        let sql = candidates("\"https://example.com/observedOn\">12:00:00Z").expect("keyed");
        assert!(sql.contains(r#""ca"."value_string" > '12:00:00.000000Z'"#), "{sql}");
        assert!(sql.contains(r#"nullif("ca"."value_kind" = (9), FALSE)"#), "{sql}");
    }

    #[test]
    fn a_date_time_range_reads_both_bounds_from_the_date_time_index() {
        let sql = candidates("\"https://example.com/observedOn\"==2024-01-01T00:00:00Z..2024-02-01T00:00:00Z").expect("keyed");
        assert!(sql.contains(r#""ca"."value_date_time" >= '2024-01-01 00:00:00.000000 +00:00'"#), "{sql}");
        assert!(sql.contains(r#""ca"."value_date_time" <= '2024-02-01 00:00:00.000000 +00:00'"#), "{sql}");
        assert!(!sql.contains("UNION ALL"), "{sql}");
    }

    /// Clause 4.22: a past-due instance holds no value, so no branch offers it.
    #[test]
    fn every_branch_skips_an_expired_instance() {
        let sql = candidates("\"https://example.com/observedOn\">2024-01-15T12:00:00Z").expect("keyed");
        assert_eq!(sql.matches(r#""ca"."expires_at" IS NULL"#).count(), 1, "{sql}");
    }

    /// A number orders through its typed column alone, which the planner
    /// already drives from `idx_ca_value_number`; a String orders under the
    /// ICU root collation, which no index orders.
    #[test]
    fn an_ordering_no_index_orders_better_than_the_planner_is_not_keyed() {
        for q in [
            "\"https://example.com/reading\">990",
            "\"https://example.com/reading\"==1..5",
            "\"https://example.com/reading\">\"23:58:00\"",
            "\"https://example.com/reading\"==2024-01-01..\"z\"",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn an_excluded_range_or_a_value_list_is_not_keyed() {
        for q in [
            "\"https://example.com/observedOn\"!=2024-01-01..2024-02-01",
            "\"https://example.com/observedOn\"==2024-01-01,2024-02-01",
            "\"https://example.com/observedOn\"==2024-01-01",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn an_ordering_on_a_sub_attribute_a_member_or_an_unrecorded_attribute_is_not_keyed() {
        for q in [
            "\"https://example.com/observedOn\".\"https://example.com/source\">2024-01-15",
            "\"https://example.com/observedOn\"[at]>2024-01-15",
            "\"https://example.com/unrecorded\">2024-01-15",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }
}
