//! Lifting a `q` comparison against a value read out of JSONB into a
//! comparable domain.

use crate::{
    declared_operand::{boolean_reading, number_reading, string_reading},
    temporal_operand::{declared_date, declared_date_time, declared_time},
};
use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::reading::{operand::Operand, target_kind::TargetKind};
use andromeda_postgresql_seaquery::{cast::cast, json_type::jsonb_whole_text, pg_type::PostgresType};
use sea_query::{Expr as SeaExpr, SimpleExpr};

/// Both sides of one document comparison, lifted into the domain the operand's
/// kind is ordered in.
///
/// A value read out of JSONB is JSON, so neither side can be compared until
/// both have been lifted the same way. Without that a numeric comparison
/// orders lexicographically and `9` exceeds `10`.
pub struct LiftedComparison {
    /// The stored value, read out of the document and lifted.
    pub stored: SimpleExpr,
    /// The bound operand, lifted the same way.
    pub operand: SimpleExpr,
}

/// Lift a stored JSONB value and one `q` operand into a common domain.
///
/// `stored` is a JSONB expression, `datatype` the expanded datatype IRI the
/// value is declared as (`NULL` when none), and the operand carries its own
/// bound value, so sea-query numbers both sides' placeholders when the
/// enclosing statement renders.
///
/// The domains, by operand kind:
///
/// * a number reads a JSON number or a JSON string declared as an XSD number,
///   and a boolean a JSON boolean or a JSON string declared as `xsd:boolean`,
///   each cast on both sides,
/// * a text operand, quoted or a bare URI, reads only a JSON string its
///   datatype gives no other data type, as text, a URI in its written
///   spelling, since a JSON value read here is a Property's,
/// * a `DateTime`, `Date` or `Time` compares only with a value declared as one
///   of its form (clause 4.9: no equality of data types, no match), a
///   `DateTime` as `timestamptz` bound as the instant itself, a `Time` in the
///   fixed-width encoding the operand is rendered in, a `Date` as its one
///   text, which already sorts chronologically.
///
/// Numbers bind directly as `NUMERIC` and `DateTime` values as `timestamptz`;
/// booleans bind as text and are cast in the statement.
///
/// A stored value of another JSON type than the operand's (the String `"3310"`
/// against the number `3310`, the number `3310` against the String `"3310"`,
/// an object against a boolean) lifts to `NULL` rather than to its spelling, so
/// the comparison is unknown for that one value: clause 4.9 makes a target of
/// another data type unequal to the operand, and never makes the query fail
/// over it.
#[must_use]
pub fn lift_comparison(operand: &Operand, stored: SimpleExpr, datatype: SimpleExpr) -> LiftedComparison {
    match operand {
        Operand::Number(number) => LiftedComparison {
            stored: number_reading(stored, datatype),
            // SeaQuery owns every AST bind, so the exact decimal must be cloned
            // from the borrowed query operand.
            operand: SeaExpr::val(number.decimal().clone()),
        },
        Operand::Boolean(value) => LiftedComparison {
            stored: boolean_reading(stored, datatype),
            operand: cast(SeaExpr::val(value.to_string()), PostgresType::Boolean),
        },
        Operand::Text(text) => LiftedComparison {
            stored: string_reading(stored, datatype),
            operand: SeaExpr::val(text.as_str()),
        },
        Operand::Uri(uri) => LiftedComparison {
            stored: string_reading(stored, datatype),
            operand: SeaExpr::val(uri.against(TargetKind::String)),
        },
        Operand::DateTime(instant) => LiftedComparison {
            stored: declared_date_time(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(*instant),
        },
        Operand::Date(date) => LiftedComparison {
            stored: declared_date(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(NgsiLdDate::from(*date).to_string()),
        },
        Operand::Time(time) => LiftedComparison {
            stored: declared_time(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(NgsiLdTime::normalise(*time).to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonb::lift_comparison;
    use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, date_time::NgsiLdDateTime, time_of_day::NgsiLdTime};
    use andromeda_ngsi_ld_query::filter::reading::operand::Operand;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr as SeaExpr, Query, SelectStatement, SimpleExpr};

    /// A plain column stands in for the JSONB value real callers lift.
    fn stored() -> SimpleExpr {
        SeaExpr::col(Alias::new("stored"))
    }

    /// The column standing in for the declared datatype.
    fn datatype() -> SimpleExpr {
        SeaExpr::col(Alias::new("datatype"))
    }

    /// One lifted operand selected alone.
    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    #[test]
    fn a_number_is_lifted_to_numeric_on_both_sides_so_it_is_not_ordered_as_text() {
        let lifted = lift_comparison(&Operand::Number(9.into()), stored(), datatype());
        assert_statement_snapshot!(
            "jsonb-a-number-is-lifted-to-numeric-on-both-sides-so-it-is-not-ordered-as-text-1",
            render(lifted.stored)
        );
        assert_statement_snapshot!(
            "jsonb-a-number-is-lifted-to-numeric-on-both-sides-so-it-is-not-ordered-as-text-2",
            render(lifted.operand)
        );
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so the String `"4"` is no number: only a JSON number, or a JSON string
    /// declared as an XSD number, reaches the cast.
    #[test]
    fn a_number_lift_reads_a_json_number_or_a_string_declared_as_one() {
        assert_statement_snapshot!(
            "jsonb-a-number-lift-reads-a-json-number-or-a-string-declared-as-one",
            render(lift_comparison(&Operand::Number(4.into()), stored(), datatype()).stored)
        );
    }

    #[test]
    fn a_boolean_lift_reads_a_json_boolean_or_a_string_declared_as_one() {
        assert_statement_snapshot!(
            "jsonb-a-boolean-lift-reads-a-json-boolean-or-a-string-declared-as-one",
            render(lift_comparison(&Operand::Boolean(true), stored(), datatype()).stored)
        );
    }

    #[test]
    fn a_boolean_is_lifted_to_boolean_on_both_sides() {
        let lifted = lift_comparison(&Operand::Boolean(true), stored(), datatype());
        assert_statement_snapshot!("jsonb-a-boolean-is-lifted-to-boolean-on-both-sides-1", render(lifted.stored));
        assert_statement_snapshot!("jsonb-a-boolean-is-lifted-to-boolean-on-both-sides-2", render(lifted.operand));
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so the number `3310` is not the String `"3310"` its spelling reads as,
    /// and neither is a JSON string declared as a number.
    #[test]
    fn a_string_operand_reads_only_a_json_string_declared_as_no_other_data_type() {
        let lifted = lift_comparison(&Operand::Text("red".to_owned()), stored(), datatype());
        assert_statement_snapshot!(
            "jsonb-a-string-operand-reads-only-a-json-string-declared-as-no-other-data-type-1",
            render(lifted.stored)
        );
        assert_statement_snapshot!(
            "jsonb-a-string-operand-reads-only-a-json-string-declared-as-no-other-data-type-2",
            render(lifted.operand)
        );
    }

    /// A declared `DateTime` is an instant, which text order does not order.
    #[test]
    fn a_date_time_is_compared_as_a_timestamp_not_as_text() {
        let instant = "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid date-time").get();
        let lifted = lift_comparison(&Operand::DateTime(instant), stored(), datatype());
        assert_statement_snapshot!("jsonb-a-date-time-is-compared-as-a-timestamp-not-as-text", render(lifted.stored));
    }

    /// Truncating to whole seconds would make `>` admit an instant inside the
    /// truncated second that the client's operand excludes.
    #[test]
    fn a_date_time_operand_binds_as_the_instant_it_names() {
        let instant = "2024-01-01T00:00:00.123456Z".parse::<NgsiLdDateTime>().expect("valid date-time").get();
        let lifted = lift_comparison(&Operand::DateTime(instant), stored(), datatype());
        assert_statement_snapshot!("jsonb-a-date-time-operand-binds-as-the-instant-it-names", render(lifted.operand));
    }

    #[test]
    fn a_time_is_brought_to_the_fixed_width_encoding_on_both_sides() {
        let time = "12:00:00,5Z".parse::<NgsiLdTime>().expect("the comma compatibility form").get();
        let lifted = lift_comparison(&Operand::Time(time), stored(), datatype());
        assert_statement_snapshot!("jsonb-a-time-is-brought-to-the-fixed-width-encoding-on-both-sides-1", render(lifted.stored));
        assert_statement_snapshot!("jsonb-a-time-is-brought-to-the-fixed-width-encoding-on-both-sides-2", render(lifted.operand));
    }

    #[test]
    fn a_date_is_compared_as_text_because_its_one_spelling_already_sorts_chronologically() {
        let date = "2024-01-01".parse::<NgsiLdDate>().expect("valid full-date").get();
        let lifted = lift_comparison(&Operand::Date(date), stored(), datatype());
        assert_statement_snapshot!(
            "jsonb-a-date-is-compared-as-text-because-its-one-spelling-already-sorts-chronologically-1",
            render(lifted.stored)
        );
        assert_statement_snapshot!(
            "jsonb-a-date-is-compared-as-text-because-its-one-spelling-already-sorts-chronologically-2",
            render(lifted.operand)
        );
    }
}
