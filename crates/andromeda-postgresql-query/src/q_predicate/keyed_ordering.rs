//! The index path of an ordering (`>`, `>=`, `<`, `<=`) or a range (`==a..b`)
//! on an attribute's own value whose operand is a number, a `DateTime`, a
//! `Date` or a `Time`.
//!
//! Such a term compiles to its typed column and, where an operand may match a
//! `languageMap` string, a language arm ([`OwnValueArms`]) that has no index,
//! so the candidates read each arm on its own: the typed arm as a range scan
//! of `idx_ca_value_number`, of `idx_ca_value_date_time` or, for a `Date` and
//! a `Time`, which are held as fixed-width text under their value kinds, of
//! `idx_ca_value_string`; any language arm through the attribute's
//! `LanguageProperty` rows.
//!
//! No other operand is offered. A String orders under the ICU root collation
//! (clause 4.9 recommends the UCA), which the byte-ordered
//! `idx_ca_value_string` cannot range over, and the clause 4.9 grammar
//! compares no boolean or URI by order.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::range_arms,
        instance_row::InstanceRow,
        keyed_branches::{branch, language_property_rows, union_of},
        ordering_term::ordering_arms,
        own_value_arms::OwnValueArms,
    },
};
use andromeda_ngsi_ld_query::filter::reading::{operand::Operand, scalar_test::ScalarTest};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, SelectStatement};

/// Whether a value index orders an operand the way the term compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandOrder {
    /// The typed column's index orders the operand's kind as the term does.
    Indexed,
    /// No index orders the operand's kind as the term does.
    Unindexed,
}

/// The order a value index gives `operand`.
const fn operand_order(operand: &Operand) -> OperandOrder {
    match operand {
        Operand::Number(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => OperandOrder::Indexed,
        Operand::Boolean(_) | Operand::Text(_) | Operand::Uri(_) => OperandOrder::Unindexed,
    }
}

/// `SELECT "entity_key"` over the rows of `attribute` whose own value passes
/// `test`, asserted; `None` unless `test` is an ordering or a range with
/// indexed operands and the attribute was recorded.
///
/// The bounds of a range are of one kind, so the lower one says how both
/// are ordered. A range outside its bounds (`!=a..b`) is a complement, which
/// never narrows: an Entity holds when its value lies outside, which an index
/// of values inside cannot enumerate.
pub(crate) fn ordering_candidates(attribute: AttributeNameKey, test: &ScalarTest, rows: &EntityRowRefs) -> Option<SelectStatement> {
    let row = InstanceRow::Current;
    let (operand, arms) = match test {
        ScalarTest::Ordered { relation, operand } => (operand, ordering_arms(*relation, operand, row)),
        ScalarTest::Within(interval) => (interval.min(), range_arms(interval, row)),
        ScalarTest::EqualsAny(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable => return None,
    };
    match operand_order(operand) {
        OperandOrder::Indexed => {}
        OperandOrder::Unindexed => return None,
    }
    let OwnValueArms { typed, language } = arms;
    let typed = branch(rows, attribute, None, typed);
    let language = language.map(|language| branch(rows, attribute, Some(language_property_rows()), Condition::all().add(language)));
    union_of([typed].into_iter().chain(language))
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
                [("observedOn", 21), ("source", 7), ("reading", 30)]
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

    /// A `languageMap` holds no time value, so the one branch ranges over the
    /// date-time index.
    #[test]
    fn a_date_time_ordering_reads_the_date_time_index_alone() {
        let statement = candidates("\"https://example.com/observedOn\">2024-01-15T12:00:00Z").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-date-time-ordering-reads-the-date-time-index-alone", statement);
    }

    /// A `Date` is held as its fixed-width text under value kind 8, whose
    /// byte order is chronological, so its branch ranges over the string index.
    #[test]
    fn a_date_ordering_reads_the_string_index_by_its_text() {
        let statement = candidates("\"https://example.com/observedOn\"<=2024-01-15").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-date-ordering-reads-the-string-index-by-its-text", statement);
    }

    /// A `Time` is held in its fixed-width storage encoding under value kind 9.
    #[test]
    fn a_time_ordering_reads_the_string_index_by_its_fixed_width_encoding() {
        let statement = candidates("\"https://example.com/observedOn\">12:00:00Z").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-time-ordering-reads-the-string-index-by-its-fixed-width-encoding", statement);
    }

    #[test]
    fn a_date_time_range_reads_both_bounds_from_the_date_time_index() {
        let statement = candidates("\"https://example.com/observedOn\"==2024-01-01T00:00:00Z..2024-02-01T00:00:00Z").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-date-time-range-reads-both-bounds-from-the-date-time-index", statement);
    }

    /// Clause 4.22: a past-due instance holds no value, so no branch offers it.
    #[test]
    fn every_branch_skips_an_expired_instance() {
        let statement = candidates("\"https://example.com/observedOn\">2024-01-15T12:00:00Z").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-every-branch-skips-an-expired-instance", statement);
    }

    /// `NaN` is unordered, so the range leaves it out of `>` as the term does.
    #[test]
    fn a_number_ordering_reads_the_number_index() {
        let statement = candidates("\"https://example.com/reading\">990").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-number-ordering-reads-the-number-index", statement);
    }

    #[test]
    fn a_number_range_reads_both_bounds_from_the_number_index() {
        let statement = candidates("\"https://example.com/reading\"==1..5").expect("keyed");
        assert_statement_snapshot!("keyed-ordering-a-number-range-reads-both-bounds-from-the-number-index", statement);
    }

    /// A String orders under the ICU root collation, which no index orders,
    /// and a range with bounds of two kinds holds nothing.
    #[test]
    fn a_string_ordering_or_a_mixed_range_is_not_keyed() {
        for q in [
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
