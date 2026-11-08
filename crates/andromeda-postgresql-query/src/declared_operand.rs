//! Lifting a declared number or Boolean read out of JSONB into the domain
//! clause 4.9 compares it in.
//!
//! A JSON string declared as an XSD number is a number and one declared as
//! `xsd:boolean` a Boolean, so a number operand reads a JSON number or such a
//! string, a Boolean operand a JSON boolean or such a string, and a String
//! operand only a JSON string its datatype gives no other data type.
//! `datatype` is the expanded datatype IRI the value is declared as, `NULL`
//! when it declares none.
//!
//! A declared value is committed in its served spelling, which the
//! `numeric` and `boolean` input functions read. The casts are still guarded by
//! `pg_input_is_valid`, so a member that does not read declines to match that
//! one row rather than failing the statement. `NaN` is unordered against every
//! operand, while PostgreSQL orders it above every `numeric`, so the number
//! reading leaves it out: it then fails `==` and every ordering, and the total
//! negation `!=` compiles to holds for it.

use crate::temporal_operand::declares_time_value;
use andromeda_ngsi_ld_lexicon::datatype::declared_value::{BOOLEAN_DATATYPE, NUMERIC_DATATYPES};
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    datetime::pg_input_is_valid,
    json_type::{JsonType, jsonb_type_is, jsonb_whole_text},
    literal::text_literal,
    operator::negate_total,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};

/// Whether `datatype` names an XSD numeric datatype; `NULL` when `datatype` is
/// `NULL`.
#[must_use]
pub fn declares_number(datatype: SimpleExpr) -> SimpleExpr {
    datatype.is_in(NUMERIC_DATATYPES.map(|iri| text_literal(iri.as_str())))
}

/// Whether `datatype` names `xsd:boolean`; `NULL` when `datatype` is `NULL`.
#[must_use]
pub fn declares_boolean(datatype: SimpleExpr) -> SimpleExpr {
    datatype.eq(text_literal(BOOLEAN_DATATYPE.as_str()))
}

/// Whether `datatype` gives a JSON string a data type other than String: a
/// time value, a number or a Boolean. `NULL` when `datatype` is `NULL`.
#[must_use]
pub fn declares_other_data_type(datatype: SimpleExpr) -> SimpleExpr {
    // clone: each test reads the one datatype
    declares_time_value(datatype.clone())
        .or(declares_number(datatype.clone()))
        .or(declares_boolean(datatype))
}

/// `stored` read as a `numeric`: a JSON number, or a JSON string declared as
/// an XSD number other than `NaN`; `NULL` otherwise.
#[must_use]
pub fn number_reading(stored: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    let text = jsonb_whole_text(stored.clone()); // clone: the tests and the cast read one value
    let declared = jsonb_type_is(stored.clone(), JsonType::String)
        .and(declares_number(datatype))
        .and(text.clone().ne(text_literal("NaN")))
        .and(pg_input_is_valid(text.clone(), PostgresType::Numeric));
    CaseStatement::new()
        .case(jsonb_type_is(stored, JsonType::Number), cast_compound(text.clone(), PostgresType::Numeric))
        .case(declared, cast_compound(text, PostgresType::Numeric))
        .into()
}

/// `stored` read as a `boolean`: a JSON boolean, or a JSON string declared as
/// `xsd:boolean`; `NULL` otherwise.
#[must_use]
pub fn boolean_reading(stored: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    let text = jsonb_whole_text(stored.clone()); // clone: the tests and the cast read one value
    let declared = jsonb_type_is(stored.clone(), JsonType::String)
        .and(declares_boolean(datatype))
        .and(pg_input_is_valid(text.clone(), PostgresType::Boolean));
    CaseStatement::new()
        .case(jsonb_type_is(stored, JsonType::Boolean), cast_compound(text.clone(), PostgresType::Boolean))
        .case(declared, cast_compound(text, PostgresType::Boolean))
        .into()
}

/// `stored` read as text: a JSON string its datatype gives no other data type;
/// `NULL` otherwise.
#[must_use]
pub fn string_reading(stored: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    let string = jsonb_type_is(stored.clone(), JsonType::String).and(negate_total(declares_other_data_type(datatype))); // clone: the guard reads the value the cast then reads
    CaseStatement::new()
        .case(string, cast_compound(jsonb_whole_text(stored), PostgresType::Text))
        .into()
}

#[cfg(test)]
mod tests {
    use crate::declared_operand::{boolean_reading, number_reading, string_reading};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn stored() -> SimpleExpr {
        Expr::col(Alias::new("stored"))
    }

    fn datatype() -> SimpleExpr {
        Expr::col(Alias::new("datatype"))
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_number_reading_takes_a_json_number_or_a_string_declared_as_one() {
        let sql = render(number_reading(stored(), datatype()));
        assert!(
            sql.contains(r#"WHEN (jsonb_typeof("stored") = ('number')) THEN ("stored" #>> ('{}'))::numeric"#),
            "{sql}"
        );
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#integer'"), "{sql}");
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#double'"), "{sql}");
        assert!(sql.contains(r#"("stored" #>> ('{}')) <> ('NaN')"#), "{sql}");
        assert!(sql.contains("pg_input_is_valid"), "{sql}");
    }

    #[test]
    fn a_boolean_reading_takes_a_json_boolean_or_a_string_declared_as_one() {
        let sql = render(boolean_reading(stored(), datatype()));
        assert!(sql.contains(r#"WHEN (jsonb_typeof("stored") = ('boolean'))"#), "{sql}");
        assert!(sql.contains(r#""datatype" = ('http://www.w3.org/2001/XMLSchema#boolean')"#), "{sql}");
    }

    #[test]
    fn a_string_reading_leaves_out_every_string_declared_as_another_data_type() {
        let sql = render(string_reading(stored(), datatype()));
        assert!(sql.contains("NOT"), "{sql}");
        assert!(sql.contains("https://uri.etsi.org/ngsi-ld/DateTime"), "{sql}");
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#decimal'"), "{sql}");
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#boolean'"), "{sql}");
    }
}
