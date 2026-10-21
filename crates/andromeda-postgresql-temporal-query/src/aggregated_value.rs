//! The operand each aggregate of clause 4.5.19 folds, built from the typed
//! value columns of one `attribute_history` row.
//!
//! Tables 4.5.19.1-1 and 4.5.19.1-2 do not give one operand to every method.
//! A JSON Array has a `sum` of its sizes and no `sumsq` of them at all, so the
//! expression an arithmetic square folds is narrower than the one an average
//! folds, and each is built here rather than at the call site that names the
//! method.
//!
//! Table 4.5.19.1-3 narrows them all again, and does it by attribute family
//! rather than by column: a Relationship has no `avg`, `min`, `max`, `stddev`,
//! `sum` or `sumsq` at all, and clause 4.5.22.1 makes `ListRelationship` "a
//! specialized type of Relationship" so the same row governs it. Every operand
//! that reads a value is therefore gated on the family before it reads a
//! column, which is what stops a `ListRelationship`'s `objectList` (a JSON
//! array in storage) from answering `avg` with the mean number of targets.
//!
//! Every numeric branch is `NUMERIC`, the type the value column itself carries:
//! a JSON number is an exact decimal (clause 4.5.2 gives no precision limit),
//! and widening it to binary floating point to aggregate would report a minimum
//! or a maximum that was never observed.

use crate::{alias::Ah, structured_json::structured_json};
use andromeda_ngsi_ld_lexicon::time::time_of_day::NgsiLdTime;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_array_length, jsonb_typeof},
    literal::{int_literal, text_literal},
    operator::regex_match,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, Expr, ExprTrait, Func, SimpleExpr};

/// The JSON type name `jsonb_typeof` reports for an array.
const JSON_ARRAY_TYPE: &str = "array";

/// The families whose every value-derived aggregate the tables mark N/A.
///
/// Table 4.5.19.1-3 gives a Relationship `distinctCount` over its targets and
/// `totalCount` over its updates and marks `avg`, `max`, `min`, `stddev`, `sum`
/// and `sumsq` N/A; clause 4.5.22.1 extends that row to `ListRelationship` by
/// making it "a specialized type of Relationship". A `VocabProperty` names
/// vocabulary members by IRI (clause 4.5.20.2), so it falls in the "URI" column
/// of Table 4.5.19.1-2, whose six value-derived cells are N/A as well.
const UNAGGREGATED_FAMILIES: [AttributeKind; 3] = [AttributeKind::Relationship, AttributeKind::ListRelationship, AttributeKind::VocabProperty];

/// Whether the row's family has any value-derived aggregate at all.
fn family_has_value_aggregates() -> SimpleExpr {
    Expr::col((Ah, AttributeHistory::AttributeKind)).is_not_in(UNAGGREGATED_FAMILIES.map(|family| int_literal(i64::from(family.code()))))
}

/// The operand, or SQL NULL for a row whose family has no value-derived
/// aggregate.
///
/// A row that falls out this way is outside every arithmetic aggregate rather
/// than contributing a zero, exactly as an object falls out of the array-size
/// operand: `AVG`, `MIN`, `MAX`, `SUM` and `STDDEV` all skip nulls.
fn only_when_aggregable(operand: SimpleExpr) -> SimpleExpr {
    CaseStatement::new().case(family_has_value_aggregates(), operand).into()
}

/// A boolean value as the number Table 4.5.19.1-1's note gives it.
///
/// The note reads true as 1 and false as 0 for the purpose of aggregation,
/// which is what makes every arithmetic method of the JSON Number column carry
/// over to the JSON Boolean column beside it.
fn boolean_as_numeric() -> SimpleExpr {
    cast(
        cast(Expr::col((Ah, AttributeHistory::ValueBoolean)), PostgresType::Integer),
        PostgresType::Numeric,
    )
}

/// The size of an array value, and nothing for a value that is not an array.
///
/// The JSONB column holds both a JSON Object and a JSON Array; Table
/// 4.5.19.1-1 measures the array by its size and gives the object no
/// value-derived aggregate at all, so an object yields SQL NULL and falls out
/// of every aggregate rather than counting as a size of zero.
fn array_size() -> SimpleExpr {
    CaseStatement::new()
        .case(
            Expr::expr(jsonb_typeof(structured_json())).eq(JSON_ARRAY_TYPE),
            cast(jsonb_array_length(structured_json()), PostgresType::Numeric),
        )
        .into()
}

/// The operand of `avg`, `min`, `max` and `sum`: a number, a boolean read as
/// its zero or one, or the size of an array.
///
/// Table 4.5.19.1-1 defines all four over the JSON Number, JSON Boolean and
/// JSON Array columns, and defines the array cases over "the sizes of the
/// arrays inside the period" rather than over the arrays themselves. Table
/// 4.5.19.1-3 defines none of them for a Relationship, whose targets are held
/// in that same JSONB column once there is more than one of them, so the family
/// gate is what keeps the array-size branch to actual JSON Array values.
#[must_use]
pub fn sized_numeric_expr() -> SimpleExpr {
    only_when_aggregable(Func::coalesce([Expr::col((Ah, AttributeHistory::ValueNumber)), boolean_as_numeric(), array_size()]).into())
}

/// The operand of `sumsq` and `stddev`: a number or a boolean read as its zero
/// or one, and never the size of an array.
///
/// Table 4.5.19.1-1 marks both methods N/A in the JSON Array column while
/// defining them in the JSON Number and JSON Boolean columns. Folding an array
/// size in here would answer a request the table refuses with the standard
/// deviation of a set of lengths.
#[must_use]
pub fn arithmetic_numeric_expr() -> SimpleExpr {
    only_when_aggregable(Func::coalesce([Expr::col((Ah, AttributeHistory::ValueNumber)), boolean_as_numeric()]).into())
}

/// The text column `min` and `max` order lexicographically, for the families
/// that have a `min` and a `max` at all.
///
/// A single-valued `VocabProperty` shares this column with the JSON Strings,
/// `Date`s and `Time`s that do have an ordering, and the "URI" column of Table
/// 4.5.19.1-2 marks its extrema N/A, so the family decides whether the column
/// is read.
#[must_use]
pub fn ordered_text_expr() -> SimpleExpr {
    only_when_aggregable(Expr::col((Ah, AttributeHistory::ValueString)))
}

/// The instant column `min` and `max` order chronologically, for the families
/// that have a `min` and a `max` at all.
#[must_use]
pub fn ordered_instant_expr() -> SimpleExpr {
    only_when_aggregable(Expr::col((Ah, AttributeHistory::ValueDateTime)))
}

/// The mean time of day of a `Time`-valued Property, in canonical form.
///
/// Table 4.5.19.1-2 defines `avg` over the Time column as "the average time
/// inside the period", which is a time of day rather than a count, so it is
/// averaged as an interval and rendered back into the clause 4.6.3 `Time`
/// lexical form. The numeric `avg` beside it stays SQL NULL for this shape, and
/// the decoder picks the column the group's recorded shape names -- the same
/// arrangement `min` and `max` already use across their three columns.
#[must_use]
pub fn average_time_expr() -> SimpleExpr {
    // The text column also holds JSON Strings and clause 4.6.3 `Date` values,
    // and casting one of those to `time` is a hard database error rather than a
    // NULL. Only a value in the canonical `Time` rendering is folded, exactly as
    // `array_size` folds only a JSONB array, so every other row falls out of the
    // aggregate instead of failing the statement.
    let time_of_day: SimpleExpr = CaseStatement::new()
        .case(
            regex_match(Expr::col((Ah, AttributeHistory::ValueString)), text_literal(NgsiLdTime::STORED_PATTERN)).and(family_has_value_aggregates()),
            cast(cast(Expr::col((Ah, AttributeHistory::ValueString)), PostgresType::Time), PostgresType::Interval),
        )
        .into();
    cast(Func::avg(time_of_day).into(), PostgresType::Time)
}

/// The single text projection `distinctCount` counts unique values of, folding
/// every typed value column into one comparable text.
///
/// Every table's `distinctCount` row reads "Calculate the count of distinct
/// values inside the period" with no column excepted (Table 4.5.19.1-3's row
/// reads "the count of distinct relationships targets inside the period"), so
/// the fold covers every value column and is never gated on the family.
#[must_use]
pub fn distinct_value_expr() -> SimpleExpr {
    Func::coalesce([
        cast(Expr::col((Ah, AttributeHistory::ValueNumber)), PostgresType::Text),
        Expr::col((Ah, AttributeHistory::ValueString)),
        cast(Expr::col((Ah, AttributeHistory::ValueBoolean)), PostgresType::Text),
        cast(Expr::col((Ah, AttributeHistory::ValueDateTime)), PostgresType::Text),
        Expr::col((Ah, AttributeHistory::ValueUri)),
        cast(Expr::col((Ah, AttributeHistory::ValueObject)), PostgresType::Text),
    ])
    .into()
}

#[cfg(test)]
mod tests {
    use crate::aggregated_value::{
        arithmetic_numeric_expr,
        average_time_expr,
        distinct_value_expr,
        ordered_instant_expr,
        ordered_text_expr,
        sized_numeric_expr,
    };
    use andromeda_postgresql_codes::attribute_kind::AttributeKind;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
    use std::collections::BTreeSet;

    fn rendered(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The family codes one rendered operand excludes from its value fold.
    fn gated_families(sql: &str) -> BTreeSet<i16> {
        let gate = sql
            .split_once("\"ah\".\"attribute_kind\" NOT IN (")
            .expect("every value operand gates on the attribute family")
            .1;
        gate.split_once(')')
            .expect("a closed family list")
            .0
            .split(',')
            .map(|code| code.trim().parse::<i16>().expect("a family code"))
            .collect()
    }

    /// Table 4.5.19.1-1, "JSON Array" column: `avg`, `min`, `max` and `sum`
    /// are taken over the sizes of the arrays, so their operand measures one.
    #[test]
    fn the_sized_operand_measures_an_array_by_its_length() {
        let sql = rendered(sized_numeric_expr());
        let structured = "COALESCE((CASE WHEN ((\"ah\".\"value_object\" ->> ('@type')) = ('@json'))";
        assert!(sql.contains(&format!("jsonb_array_length({structured}")), "{sql}");
        assert!(sql.contains(&format!("jsonb_typeof({structured}")), "{sql}");
    }

    /// Table 4.5.19.1-1 marks `stddev` and `sumsq` N/A for a JSON Array, so
    /// their operand must not reach the JSONB column at all.
    #[test]
    fn the_arithmetic_operand_never_measures_an_array() {
        let sql = rendered(arithmetic_numeric_expr());
        assert!(!sql.contains("jsonb_array_length"), "{sql}");
        assert!(!sql.contains("value_object"), "{sql}");
    }

    /// Table 4.5.19.1-1's note reads true as 1 and false as 0, which is what
    /// carries every arithmetic method over to the JSON Boolean column.
    #[test]
    fn both_numeric_operands_read_a_boolean_as_its_zero_or_one() {
        for sql in [rendered(sized_numeric_expr()), rendered(arithmetic_numeric_expr())] {
            assert!(sql.contains("\"ah\".\"value_boolean\"::integer::numeric"), "{sql}");
        }
    }

    /// An exact decimal stays exact: no operand widens the value column to
    /// binary floating point on its way into an aggregate.
    #[test]
    fn no_operand_widens_the_exact_value_column_to_a_float() {
        for sql in [rendered(sized_numeric_expr()), rendered(arithmetic_numeric_expr())] {
            assert!(!sql.contains("float8"), "{sql}");
            assert!(!sql.contains("double precision"), "{sql}");
        }
    }

    /// Table 4.5.19.1-3 marks every value-derived method N/A for a
    /// Relationship, clause 4.5.22.1 extends that to `ListRelationship`, and
    /// Table 4.5.19.1-2's "URI" column does the same for a `VocabProperty`'s
    /// IRIs. Every operand those methods fold therefore refuses the three
    /// families outright rather than measuring whatever column their targets
    /// happened to land in.
    #[test]
    fn no_value_operand_folds_a_family_the_tables_mark_not_applicable() {
        let refused: BTreeSet<i16> = [AttributeKind::Relationship, AttributeKind::ListRelationship, AttributeKind::VocabProperty]
            .iter()
            .map(|family| family.code())
            .collect();
        for sql in [
            rendered(sized_numeric_expr()),
            rendered(arithmetic_numeric_expr()),
            rendered(ordered_text_expr()),
            rendered(ordered_instant_expr()),
            rendered(average_time_expr()),
        ] {
            pretty_assertions::assert_eq!(gated_families(&sql), refused, "{sql}");
        }
    }

    /// The families the tables do give value-derived aggregates to are not
    /// named by the gate, so a Property, `GeoProperty`, `LanguageProperty`,
    /// `JsonProperty` or `ListProperty` still folds its value. Asserted as the
    /// exact set the gate names, so a family added to it later cannot pass
    /// unnoticed.
    #[test]
    fn the_family_gate_names_only_the_three_families_the_tables_refuse() {
        let gated = gated_families(&rendered(sized_numeric_expr()));
        for family in [
            AttributeKind::Property,
            AttributeKind::GeoProperty,
            AttributeKind::LanguageProperty,
            AttributeKind::JsonProperty,
            AttributeKind::ListProperty,
        ] {
            assert!(!gated.contains(&family.code()), "{family:?} must not be gated out: {gated:?}");
        }
    }

    /// `distinctCount` is defined in every column of every table, including
    /// Table 4.5.19.1-3's "count of distinct relationships targets", so its
    /// fold reaches every typed value column and is never gated on the family.
    #[test]
    fn the_distinct_operand_folds_every_typed_value_column_for_every_family() {
        let sql = rendered(distinct_value_expr());
        for column in ["value_number", "value_string", "value_boolean", "value_date_time", "value_uri", "value_object"] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
        assert!(!sql.contains("attribute_kind"), "counting must not be gated on the family: {sql}");
    }
}
