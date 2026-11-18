//! Which typed fields of a stored instance a clause 4.9 comparison reads, and
//! how it compares one operand against each.
//!
//! A comparison reads every field holding a stored kind whose target kind the
//! operand compares with ([`compared_kinds`]), so the data type rule is the
//! reading's, and where each kind is held is the placement the write path
//! stores by ([`value_placement`]):
//!
//! * numbers in `value_number` (`numeric`), bound directly as an exact
//!   decimal, so every supported operand uses the typed numeric index; a JSON
//!   string declared as an XSD number lands there too, under its own value
//!   kind, `INF`, `-INF` and `NaN` included,
//! * booleans in `value_boolean`, a JSON string declared as `xsd:boolean`
//!   beside them,
//! * date-times in `value_date_time`,
//! * strings in `value_string` for Properties and single-term
//!   `VocabProperties` (or, past the B-tree bound, in `value_long_text`, which
//!   [`ValueColumn::holding`] decides from the byte length alone) and the
//!   object of a Relationship in `value_uri` (or, past the B-tree bound, in
//!   `value_long_uri`, decided the same way), which clause 4.9 compares under
//!   equality alone,
//! * values declared as a `Date` or a `Time` in `value_string` too, told apart
//!   from a String by `value_kind`. A date has one spelling, and a time is
//!   held in its fixed-width storage encoding, so byte-wise order over either
//!   is chronological order: every operator compares the bound operand text
//!   against the column, which its index serves. Both are always short, so
//!   they never read `value_long_text`.
//!
//! A field that holds kinds of several data types (`value_string`) is read
//! beside the value kind of the one compared, since clause 4.9 matches a
//! target only with an operand of its own data type.
//!
//! Every value whose family is not a single scalar lives in `value_object`
//! instead; [`crate::q_predicate::structured_target`] compares those.

use crate::{
    operator::compare,
    q_predicate::{
        instance_row::InstanceRow,
        stored_kind::{compared_kinds, stored_target_kinds},
        string_ordering::ordered_string,
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
use andromeda_ngsi_ld_number::declared_number::NonFinite;
use andromeda_ngsi_ld_query::filter::reading::{comparator::Comparator, operand::Operand, target_kind::TargetKind};
use andromeda_postgresql_seaquery::{
    literal::false_literal,
    numeric_special::{NumericSpecial, is_not_special, is_special},
};
use andromeda_postgresql_stored_values::{
    value_kind::ValueKind,
    value_placement::{ToldApart, ValueField, ValuePlacement, value_placement},
};
use sea_query::{Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Compare one operand by `comparator` against every typed field holding a
/// value of a kind it compares with.
///
/// Every arm names one typed field and compares it to a bound value, so each
/// comparison is the shape the partial index over that field is predicated
/// on: `value_number > $1` implies `value_number IS NOT NULL`, which is what
/// makes `(attribute_name_key, value_number) WHERE value_number IS NOT NULL`
/// applicable. The row's check constraint ties the kind to the populated
/// field, so a field being non-NULL is the kind test for every field but
/// `value_string`, which three kinds share and which is therefore read beside
/// its `value_kind`.
///
/// A row whose value lives in another field therefore yields NULL, not FALSE,
/// which is the reading clause 4.9 wants of a comparison. Equality and the
/// orderings state it the same way: "If there is no equality between the
/// target value data type and the Query Term value data type, then it shall
/// be considered as not matching", and an unknown that the enclosing `EXISTS`
/// drops is exactly that. Unequal reads a datatype difference the other way,
/// and no comparison here is unequal: `!=` is the complement of the whole
/// equality, which the verdict condition states once.
#[must_use]
pub fn typed_column_condition(operand: &Operand, comparator: Comparator, row: InstanceRow) -> SimpleExpr {
    typed_field_conditions(operand, comparator, row)
        .into_iter()
        .reduce(ExprTrait::or)
        .unwrap_or_else(false_literal)
}

/// The arms of [`typed_column_condition`], one comparison per typed field it
/// reads, each answered by that field's own index.
#[must_use]
pub fn typed_field_conditions(operand: &Operand, comparator: Comparator, row: InstanceRow) -> Vec<SimpleExpr> {
    field_arms(operand, comparator)
        .iter()
        .map(|arm| arm.condition(operand, comparator, row))
        .collect()
}

/// `compared`, a test of a String one of `row`'s String columns holds, read
/// for the String kind alone where that field also holds kinds of another data
/// type: a `Date` and a `Time` share it, and clause 4.9 makes no target of
/// another data type match a String operator.
#[must_use]
pub(crate) fn as_string(compared: SimpleExpr, row: InstanceRow) -> SimpleExpr {
    match value_placement(ValueKind::String) {
        ValuePlacement::Field {
            told_apart: ToldApart::ByValueKind,
            ..
        } => compared.and(row.value_kind_is(ValueKind::String)),
        ValuePlacement::Field {
            told_apart: ToldApart::Unneeded,
            ..
        } => compared,
        ValuePlacement::Proof | ValuePlacement::Valueless => false_literal(),
    }
}

/// Whether the instance's own value is a String in its String field: told
/// apart by its value kind where the field also holds kinds of another data
/// type.
#[must_use]
pub(crate) fn holds_string(row: InstanceRow) -> SimpleExpr {
    match value_placement(ValueKind::String) {
        ValuePlacement::Field {
            told_apart: ToldApart::ByValueKind,
            ..
        } => row.value_kind_is(ValueKind::String),
        ValuePlacement::Field {
            field,
            told_apart: ToldApart::Unneeded,
        } => row
            .columns(field)
            .map(|column| row.value(column).is_not_null())
            .reduce(ExprTrait::or)
            .unwrap_or_else(false_literal),
        ValuePlacement::Proof | ValuePlacement::Valueless => false_literal(),
    }
}

/// One typed field a comparison reads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldArm {
    /// The field.
    field: ValueField,
    /// The kind the field is read for, where it also holds kinds of another
    /// data type; `None` where every kind it holds compares alike.
    told_apart: Option<ValueKind>,
    /// The stored kinds the field is read for.
    kinds: Vec<ValueKind>,
}

/// The fields a comparison of `operand` by `comparator` reads, in the order
/// the kinds they are read for are numbered, each once.
///
/// A seal and a tombstone are in no typed field, and no operand compares with
/// either.
fn field_arms(operand: &Operand, comparator: Comparator) -> Vec<FieldArm> {
    compared_kinds(operand, comparator.comparison()).fold(Vec::new(), |mut arms, kind| {
        let (field, told_apart) = match value_placement(kind) {
            ValuePlacement::Field {
                field,
                told_apart: ToldApart::Unneeded,
            } => (field, None),
            ValuePlacement::Field {
                field,
                told_apart: ToldApart::ByValueKind,
            } => (field, Some(kind)),
            ValuePlacement::Proof | ValuePlacement::Valueless => return arms,
        };
        match arms.iter_mut().find(|arm| arm.field == field && arm.told_apart == told_apart) {
            Some(arm) => arm.kinds.push(kind),
            None => arms.push(FieldArm {
                field,
                told_apart,
                kinds: vec![kind],
            }),
        }
        arms
    })
}

impl FieldArm {
    /// The comparison of `operand` by `comparator` on this field of `row`.
    ///
    /// The numbers beyond the decimals the field holds are answered as the
    /// reading orders them, where the comparison on the column answers them
    /// otherwise ([`beyond_decimals`]).
    fn condition(&self, operand: &Operand, comparator: Comparator, row: InstanceRow) -> SimpleExpr {
        let Some(kind) = self.compared_kind(operand, comparator) else {
            return false_literal();
        };
        let compared = field_comparison(operand, comparator, self.field, kind, row);
        let compared = self
            .kinds
            .iter()
            .flat_map(|kind| stored_target_kinds(*kind))
            .filter_map(|target| target.non_finite())
            .fold(compared, |compared, value| {
                beyond_decimals(compared, value, comparator, operand, row.value(ValueColumn::Field(self.field)))
            });
        match self.told_apart {
            Some(kind) => compared.and(row.value_kind_is(kind)),
            None => compared,
        }
    }

    /// The target kind the field is compared with `operand` as: the first of
    /// its kinds the operand compares with after `comparator`.
    fn compared_kind(&self, operand: &Operand, comparator: Comparator) -> Option<TargetKind> {
        self.kinds
            .iter()
            .flat_map(|kind| stored_target_kinds(*kind))
            .copied()
            .find(|target| operand.compares_with(*target, comparator.comparison()))
    }
}

/// `operand` compared by `comparator` with the value of `kind` `field` of
/// `row` holds, a text operand in the spelling a target of `kind` is compared
/// with.
///
/// Equality of text is byte identity under `COLLATE "C"`: a stored value equal
/// to the operand has the operand's length, so it sits in the one column the
/// operand itself would be placed in, and only that column is read. A short
/// `==` therefore compiles exactly as it would with no overflow column at all.
/// An ordering of text reads both columns, because a stored value of any
/// length may order on either side of the operand; a row whose value is in the
/// other column yields NULL for that arm, which the disjunction absorbs. It
/// compares under the collation [`ordered_string`] picks.
///
/// PostgreSQL rewrites an equality with the boolean `false` into the negated
/// column, which does not imply the column is non-NULL, so that comparison
/// states it ([`held_false`]).
fn field_comparison(operand: &Operand, comparator: Comparator, field: ValueField, kind: TargetKind, row: InstanceRow) -> SimpleExpr {
    match (operand.text_against(kind), comparator) {
        (Some(text), Comparator::Equal) => compare(row.value(ValueColumn::holding(field, text)), comparator, bound_operand(operand, kind)),
        (Some(_), Comparator::Ordered(_)) => row
            .columns(field)
            .map(|column| compare(ordered_string(row.value(column), operand, comparator), comparator, bound_operand(operand, kind)))
            .reduce(ExprTrait::or)
            .unwrap_or_else(false_literal),
        (None, Comparator::Equal) => {
            let compared = compare(row.value(ValueColumn::Field(field)), comparator, bound_operand(operand, kind));
            match operand {
                Operand::Boolean(false) => held_false(compared, row.value(ValueColumn::Field(field))),
                Operand::Boolean(true)
                | Operand::Number(_)
                | Operand::Text(_)
                | Operand::Uri(_)
                | Operand::DateTime(_)
                | Operand::Date(_)
                | Operand::Time(_) => compared,
            }
        }
        (None, Comparator::Ordered(_)) => compare(row.value(ValueColumn::Field(field)), comparator, bound_operand(operand, kind)),
    }
}

/// `compared`, an equality of the boolean `column` with `false`, stated
/// beside `column IS NOT NULL`.
///
/// The planner reads `column = false` as `NOT column`, from which it cannot
/// prove the predicate of the partial index over the column, so without the
/// stated predicate a read of a rare `false` scans every row of the attribute
/// instead of the index. `column = true` reads as the bare column, which
/// proves it.
fn held_false(compared: SimpleExpr, column: SimpleExpr) -> SimpleExpr {
    compared.and(column.is_not_null())
}

/// The value `operand` is bound as against the field holding a target of
/// `kind`.
///
/// sea-query owns every value in its AST, so a number is the one necessary
/// clone between the borrowed query domain value and the `NUMERIC` bind. A URI
/// binds in the spelling a target of `kind` is compared with, a `Date` as its
/// one spelling and a `Time` in the fixed-width encoding the string column
/// holds it in.
#[must_use]
pub fn bound_operand(operand: &Operand, kind: TargetKind) -> SimpleExpr {
    match operand {
        Operand::Number(number) => SeaExpr::val(number.decimal().clone()),
        Operand::Text(text) => SeaExpr::val(text.as_str()),
        Operand::Uri(uri) => SeaExpr::val(uri.against(kind)),
        Operand::Boolean(value) => SeaExpr::val(*value),
        Operand::DateTime(instant) => SeaExpr::val(*instant),
        Operand::Date(date) => SeaExpr::val(NgsiLdDate::from(*date).to_string()),
        Operand::Time(time) => SeaExpr::val(NgsiLdTime::normalise(*time).to_string()),
    }
}

/// `compared`, a comparison of the number `column` with `operand` by
/// `comparator`, answering `value` as the reading orders it.
///
/// A declared `xsd:float` or `xsd:double` holds `INF`, `-INF` and `NaN` as the
/// `numeric` values PostgreSQL orders against every finite operand one way
/// each ([`NumericSpecial::order_against_finite`]). Where that order makes
/// the comparison hold and the reading's does not, the value is left out, and
/// where the reading's makes it hold and that order does not, it is admitted:
/// `NaN`, which the reading leaves unordered, is left out of `>` and `>=`,
/// and the infinities need neither, since both orders place them alike.
fn beyond_decimals(compared: SimpleExpr, value: NonFinite, comparator: Comparator, operand: &Operand, column: SimpleExpr) -> SimpleExpr {
    let special = numeric_special(value);
    match (
        comparator.holds(special.order_against_finite()),
        comparator.holds_beyond_decimals(value, operand),
    ) {
        (true, false) => compared.and(is_not_special(column, special)),
        (false, true) => compared.or(is_special(column, special)),
        (true, true) | (false, false) => compared,
    }
}

/// The `numeric` value a number beyond the decimals is held as.
const fn numeric_special(value: NonFinite) -> NumericSpecial {
    match value {
        NonFinite::NotANumber => NumericSpecial::NotANumber,
        NonFinite::PositiveInfinity => NumericSpecial::Infinity,
        NonFinite::NegativeInfinity => NumericSpecial::NegativeInfinity,
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::{ElementLevel, InstanceRow},
        typed_column::typed_column_condition,
    };
    use andromeda_ngsi_ld_query::filter::{
        parser::operand::{parse_date, parse_time},
        reading::{comparator::Comparator, operand::Operand, order_relation::OrderRelation},
    };
    use andromeda_postgresql_schema::{string_value_column::INDEXED_STRING_BYTE_LIMIT, uri_value_column::INDEXED_URI_BYTE_LIMIT};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Query, SelectStatement, SimpleExpr};

    /// The four orderings, in the order the ordering operators are written.
    const ORDERINGS: [Comparator; 4] = [
        Comparator::Ordered(OrderRelation::Greater),
        Comparator::Ordered(OrderRelation::GreaterOrEqual),
        Comparator::Ordered(OrderRelation::Less),
        Comparator::Ordered(OrderRelation::LessOrEqual),
    ];

    /// The comparison over a current row.
    fn current(operand: &Operand, comparator: Comparator) -> SimpleExpr {
        typed_column_condition(operand, comparator, InstanceRow::Current)
    }

    fn text(value: impl Into<String>) -> Operand {
        Operand::Text(value.into())
    }

    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    #[test]
    fn each_operand_kind_names_the_column_the_write_path_projects_it_into() {
        for (index, operand) in [Operand::Number(1.into()), Operand::Boolean(true), text("red")].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("typed-column-each-operand-kind-names-the-column-the-write-path-projects-it-into-{}", index + 1),
                render(current(&operand, Comparator::Equal))
            );
        }
    }

    /// Clause 4.9: a Relationship operand shall non-match every operator other
    /// than equality and inequality, and `value_uri` holds Relationship objects
    /// and nothing else.
    #[test]
    fn only_the_equality_operators_reach_relationship_objects() {
        let operand = text("urn:ngsi-ld:Person:1");
        for (index, comparator) in [Comparator::Equal].into_iter().chain(ORDERINGS).enumerate() {
            assert_statement_snapshot!(
                format!("typed-column-only-the-equality-operators-reach-relationship-objects-{}", index + 1),
                render(current(&operand, comparator))
            );
        }
    }

    /// `NaN` is unordered, so every ordering on the number column leaves it
    /// out: `>` and `>=` by naming it, since PostgreSQL orders it above every
    /// number, and `<` and `<=` by that order alone. It never equals a finite
    /// operand, so equality reads the column alone, for both number kinds.
    #[test]
    fn a_number_ordering_leaves_not_a_number_out_and_equality_does_not_guard() {
        for (index, comparator) in ORDERINGS.into_iter().chain([Comparator::Equal]).enumerate() {
            assert_statement_snapshot!(
                format!(
                    "typed-column-a-number-ordering-leaves-not-a-number-out-and-equality-does-not-guard-{}",
                    index + 1
                ),
                render(current(&Operand::Number(3.into()), comparator))
            );
        }
    }

    /// The planner reads `= false` as the negated column, from which it cannot
    /// prove the partial index's predicate, so the comparison states it.
    #[test]
    fn an_equality_with_false_states_its_field_is_held() {
        assert_statement_snapshot!(
            "typed-column-an-equality-with-false-states-its-field-is-held",
            render(current(&Operand::Boolean(false), Comparator::Equal))
        );
    }

    /// RFC 3986 section 6.2.2.2: a percent-encoded reserved character is not
    /// equivalent to the character, so the operand is compared as written.
    #[test]
    fn a_percent_encoded_relationship_operand_is_compared_as_written() {
        assert_statement_snapshot!(
            "typed-column-a-percent-encoded-relationship-operand-is-compared-as-written",
            render(current(&text("urn:ngsi-ld%3ABuilding%3A1"), Comparator::Equal))
        );
    }

    /// A Relationship object past the B-tree bound is projected into
    /// `value_long_uri`, so an equality on an operand that long reads that
    /// column and never `value_uri`, where no such object can be.
    #[test]
    fn a_uri_equality_past_the_limit_reads_only_the_overflow_uri_column() {
        let long = text(format!("urn:ngsi-ld:Person:{}", "a".repeat(6000)));
        assert_statement_snapshot!(
            "typed-column-a-uri-equality-past-the-limit-reads-only-the-overflow-uri-column",
            render(current(&long, Comparator::Equal))
        );
    }

    #[test]
    fn a_uri_equality_at_the_limit_reads_only_the_indexed_uri_column() {
        let prefix = "urn:ngsi-ld:Person:";
        let at_limit = format!("{prefix}{}", "a".repeat(INDEXED_URI_BYTE_LIMIT - prefix.len()));
        let statement = render(current(&text(at_limit), Comparator::Equal));
        assert_statement_snapshot!("typed-column-a-uri-equality-at-the-limit-reads-only-the-indexed-uri-column", statement);
    }

    /// History keeps a URI of any length in `value_uri`, so a long URI
    /// equality over history reads that column.
    #[test]
    fn a_long_uri_equality_over_history_reads_the_one_history_uri_column() {
        let long = format!("urn:ngsi-ld:Person:{}", "a".repeat(6000));
        let statement = render(typed_column_condition(&text(long), Comparator::Equal, InstanceRow::History));
        assert_statement_snapshot!("typed-column-a-long-uri-equality-over-history-reads-the-one-history-uri-column", statement);
    }

    /// An element keeps a String of any length in `value_string`, so its
    /// ordering reads that field alone.
    #[test]
    fn an_element_string_ordering_reads_its_one_string_field() {
        let statement = render(typed_column_condition(
            &text("m"),
            Comparator::Ordered(OrderRelation::Greater),
            InstanceRow::Element(ElementLevel::at(0)),
        ));
        assert_statement_snapshot!("typed-column-an-element-string-ordering-reads-its-one-string-field", statement);
    }

    #[test]
    fn a_short_string_equality_reads_only_the_indexed_column() {
        let statement = render(current(&text("red"), Comparator::Equal));
        assert_statement_snapshot!("typed-column-a-short-string-equality-reads-only-the-indexed-column", statement);
    }

    #[test]
    fn a_string_equality_past_the_limit_reads_only_the_overflow_column() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let statement = render(current(&text(long), Comparator::Equal));
        assert_statement_snapshot!("typed-column-a-string-equality-past-the-limit-reads-only-the-overflow-column", statement);
    }

    #[test]
    fn string_ordering_reads_both_string_columns_and_never_relationship_objects() {
        for (index, comparator) in ORDERINGS.into_iter().enumerate() {
            assert_statement_snapshot!(
                format!(
                    "typed-column-string-ordering-reads-both-string-columns-and-never-relationship-objects-{}",
                    index + 1
                ),
                render(current(&text("m"), comparator))
            );
        }
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, so an
    /// equality keeps the byte order of the column and its index.
    #[test]
    fn a_string_equality_compares_under_the_column_byte_order() {
        assert_statement_snapshot!(
            "typed-column-a-string-equality-compares-under-the-column-byte-order",
            render(current(&text("m"), Comparator::Equal))
        );
    }

    /// History keeps a String of any length in `value_string`, so a long
    /// String equality over history reads that column.
    #[test]
    fn a_long_string_equality_over_history_reads_the_one_history_string_column() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let statement = render(typed_column_condition(&text(long), Comparator::Equal, InstanceRow::History));
        assert_statement_snapshot!(
            "typed-column-a-long-string-equality-over-history-reads-the-one-history-string-column",
            statement
        );
    }

    #[test]
    fn date_and_time_operands_never_read_the_overflow_column() {
        let date = parse_date("2024-01-15").expect("a date");
        let time = parse_time("12:00:00Z").expect("a time");
        for (scalar_index, operand) in [Operand::Date(date), Operand::Time(time)].into_iter().enumerate() {
            for (op_index, comparator) in [Comparator::Equal, ORDERINGS[0], ORDERINGS[2]].into_iter().enumerate() {
                assert_statement_snapshot!(
                    format!(
                        "typed-column-date-and-time-operands-never-read-the-overflow-column-{}-{}",
                        scalar_index + 1,
                        op_index + 1
                    ),
                    render(current(&operand, comparator))
                );
            }
        }
    }

    /// A `Time` is held in its fixed-width storage encoding under value kind
    /// 9, so every operator compares one bound text against `value_string`,
    /// which its index serves, at every depth.
    #[test]
    fn a_time_operand_compares_one_fixed_width_text_under_its_value_kind() {
        let time = parse_time("12:00:00,5Z").expect("the comma compatibility form");
        for (row_index, row) in [InstanceRow::Current, InstanceRow::Element(ElementLevel::at(0))].into_iter().enumerate() {
            for (op_index, comparator) in [Comparator::Equal, ORDERINGS[0], ORDERINGS[3]].into_iter().enumerate() {
                assert_statement_snapshot!(
                    format!(
                        "typed-column-a-time-operand-compares-one-fixed-width-text-under-its-value-kind-{}-{}",
                        row_index + 1,
                        op_index + 1
                    ),
                    render(typed_column_condition(&Operand::Time(time), comparator, row))
                );
            }
        }
    }

    #[test]
    fn a_date_operand_reads_value_kind_8() {
        let date = parse_date("2024-01-15").expect("a date");
        let statement = render(current(&Operand::Date(date), Comparator::Equal));
        assert_statement_snapshot!("typed-column-a-date-operand-reads-value-kind-8", statement);
    }

    /// Clause 4.9: a quoted operand is a String, which matches no `Date` or
    /// `Time` the string column also holds.
    #[test]
    fn a_string_operand_reads_value_kind_2() {
        for (index, comparator) in [Comparator::Equal, ORDERINGS[0]].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("typed-column-a-string-operand-reads-value-kind-2-{}", index + 1),
                render(current(&text("2024-01-15"), comparator))
            );
        }
    }
}
