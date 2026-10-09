//! The clause 4.23.2 datatype of one stored value, and where that datatype
//! ranks.
//!
//! Clause 4.23.2: "When sorting by value, and the values of Attributes are of
//! mixed datatypes, the following comparison order (null values last), shall be
//! implicitly applied: Numbers, Strings, Object, Array, Boolean, Time, Date,
//! `DateTime`, Null" and, last of all, "Attribute does not exist".
//!
//! A stored instance holds its value in the typed field of its kind, so the
//! field that is set names most datatypes. Clause 4.6.3 names three data types
//! whose whole representation *is* the string, and a `Date` or a `Time` is held
//! as its text, so a string is read off its literal: that reading is
//! [`TimeValueForm`], the classifier the write projection and the clause 4.9
//! `q` operand parser share, and the pattern below is that same classifier
//! written for a statement. A stored timestamp must not be a `DateTime` to `q`
//! and a String to the ordering.
//!
//! The order is rendered as a rank rather than as a chain of comparisons. The
//! datatype is read as a single token, and the token's position in a
//! nine-element sequence *is* its rank, so a descending sort is the same
//! expression over the reversed sequence and a value with no datatype ranks
//! nowhere at all, which, sorted `NULLS LAST`, is exactly where "attribute does
//! not exist" belongs, in both directions.

use andromeda_ngsi_ld_lexicon::time_value::TimeValueForm;
use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
use andromeda_ngsi_ld_vocabulary::expanded::classes::{DATE, DATE_TIME, TIME};
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_key_exists, jsonb_typeof},
    literal::text_literal,
    operator::regex_match,
    pg_array::{array_position, text_array},
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};

/// The JSON-LD keyword holding a value node's literal value.
pub(crate) const VALUE_MEMBER: &str = "@value";

/// The JSON-LD keyword holding the elements of an ordered list.
///
/// Clauses 5.2.24 and 5.2.25 give the two list families an ordered value, held
/// as the `@list` body, or under this keyword where a structured value nests
/// one.
const LIST_MEMBER: &str = "@list";

/// The token for a number.
pub(crate) const NUMBER_TYPE: &str = "number";

/// The token for a string.
const STRING_TYPE: &str = "string";

/// The token for a JSON object.
pub(crate) const OBJECT_TYPE: &str = "object";

/// The token, and the `jsonb_typeof` name, for a JSON array.
pub(crate) const ARRAY_TYPE: &str = "array";

/// The token for a boolean.
const BOOLEAN_TYPE: &str = "boolean";

/// The token for a JSON null.
const NULL_TYPE: &str = "null";

/// How many datatypes clause 4.23.2 ranks before "attribute does not exist".
const DATATYPE_COUNT: usize = 9;

/// The typed value one ordering term reads, one field per kind; at most one is
/// set.
#[derive(Debug, Clone)]
pub(crate) struct OrderedValue {
    /// A number.
    pub(crate) number: SimpleExpr,
    /// A boolean.
    pub(crate) boolean: SimpleExpr,
    /// A `DateTime` held as an instant.
    pub(crate) date_time: SimpleExpr,
    /// A string: a String, a Relationship's object, a `Date` or a `Time`.
    pub(crate) text: SimpleExpr,
    /// A structured value: an object or an array.
    pub(crate) object: SimpleExpr,
}

/// The one token that names the value's clause-4.23.2 datatype, `NULL` for
/// no value.
///
/// An array (several values, a list, a `languageMap`, several objects) ranks
/// as Array, and so does a structured value nesting a `@list`; any other
/// structure is an Object.
pub(crate) fn datatype_token(value: &OrderedValue) -> SimpleExpr {
    let (time, date, date_time) = (TIME, DATE, DATE_TIME);
    // clone: sea-query owns every expression in its AST, so each reading takes
    // its own copy of the columns the lateral projects once.
    let listed = jsonb_typeof(value.object.clone())
        .eq(text_literal(ARRAY_TYPE))
        .or(jsonb_key_exists(value.object.clone(), text_literal(LIST_MEMBER)));
    CaseStatement::new()
        .case(value.number.clone().is_not_null(), text_literal(NUMBER_TYPE))
        .case(value.boolean.clone().is_not_null(), text_literal(BOOLEAN_TYPE))
        .case(value.date_time.clone().is_not_null(), text_literal(date_time.as_str()))
        .case(written_in(&value.text, TimeValueForm::DateTime), text_literal(date_time.as_str()))
        .case(written_in(&value.text, TimeValueForm::Date), text_literal(date.as_str()))
        .case(written_in(&value.text, TimeValueForm::Time), text_literal(time.as_str()))
        .case(value.text.clone().is_not_null(), text_literal(STRING_TYPE))
        .case(listed, text_literal(ARRAY_TYPE))
        .case(value.object.clone().is_not_null(), text_literal(OBJECT_TYPE))
        .into()
}

/// The clause-4.23.2 rank of `datatype`, `NULL` when the entity carries no such
/// attribute.
///
/// Always sorted ascending, whichever way the term runs: the sequence the rank
/// is read from is the one that already runs the term's way, which is what keeps
/// Null and "attribute does not exist" at the end of a descending sort instead
/// of moving them to the front.
pub(crate) fn datatype_rank(datatype: SimpleExpr, direction: SortDirection) -> SimpleExpr {
    let (time, date, date_time) = (TIME, DATE, DATE_TIME);
    let sequence: [&str; DATATYPE_COUNT] = match direction {
        SortDirection::Ascending => [
            NUMBER_TYPE,
            STRING_TYPE,
            OBJECT_TYPE,
            ARRAY_TYPE,
            BOOLEAN_TYPE,
            time.as_str(),
            date.as_str(),
            date_time.as_str(),
            NULL_TYPE,
        ],
        SortDirection::Descending => [
            date_time.as_str(),
            date.as_str(),
            time.as_str(),
            BOOLEAN_TYPE,
            ARRAY_TYPE,
            OBJECT_TYPE,
            STRING_TYPE,
            NUMBER_TYPE,
            NULL_TYPE,
        ],
    };
    array_position(cast(text_array(&sequence), PostgresType::TextArray), datatype)
}

/// Whether a string is written in one clause 4.6.3 form.
///
/// The pattern is the lexical form's own, taken from the classifier the write
/// projection and the `q` operand parser read, so the three sides cannot come to
/// disagree about what a literal is.
fn written_in(text: &SimpleExpr, form: TimeValueForm) -> SimpleExpr {
    regex_match(text.clone(), text_literal(form.lexical_pattern())) // clone: the text is read by every reading
}

#[cfg(test)]
mod tests {
    use crate::attribute_value_datatype::{OrderedValue, datatype_rank, datatype_token};
    use andromeda_ngsi_ld_lexicon::time_value::TimeValueForm;
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn rendered_token() -> String {
        let column = |name: &str| Expr::col(Alias::new(name));
        let value = OrderedValue {
            number: column("n"),
            boolean: column("b"),
            date_time: column("t"),
            text: column("s"),
            object: column("o"),
        };
        Query::select().expr(datatype_token(&value)).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Clause 4.23.2: Numbers, Strings, Object, Array, Boolean, Time, Date,
    /// `DateTime`, Null. The rank is the token's position in that sequence.
    #[test]
    fn the_ascending_rank_reads_the_clause_4_23_2_sequence_in_order() {
        let sql = Query::select()
            .expr(datatype_rank(int_literal(1), SortDirection::Ascending))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("array_position("), "{sql}");
        let sequence = "'number','string','object','array','boolean','https://uri.etsi.org/ngsi-ld/Time','https://uri.etsi.org/ngsi-ld/Date','https://uri.etsi.org/ngsi-ld/DateTime','null'";
        assert!(sql.contains(sequence), "{sql}");
    }

    /// A descending sort reverses the datatype order but leaves Null where
    /// clause 4.23.2 pins it: last.
    #[test]
    fn the_descending_rank_reverses_the_datatypes_and_still_ends_with_null() {
        let sql = Query::select()
            .expr(datatype_rank(int_literal(1), SortDirection::Descending))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        let null_at = sql.find("'null'").expect("the null token is ranked");
        let number_at = sql.find("'number'").expect("the number token is ranked");
        let date_time_at = sql.find("'https://uri.etsi.org/ngsi-ld/DateTime'").expect("the DateTime token is ranked");
        assert!(date_time_at < number_at, "the datatypes reverse: {sql}");
        assert!(number_at < null_at, "null stays last: {sql}");
    }

    /// A `Date` and a `Time` are held as their text, so the literal is what
    /// says which it is, read by the lexical form's own pattern before the
    /// plain-string reading.
    #[test]
    fn a_string_is_classified_by_its_clause_4_6_3_form_before_it_is_a_string() {
        let sql = rendered_token();
        let mut last = 0;
        for pattern in [
            TimeValueForm::DateTime.lexical_pattern(),
            TimeValueForm::Date.lexical_pattern(),
            TimeValueForm::Time.lexical_pattern(),
        ] {
            let at = sql.find(pattern).unwrap_or_else(|| panic!("{pattern} is not read: {sql}"));
            assert!(at > last, "{sql}");
            last = at;
        }
        let fallback = sql.find("\"s\" IS NOT NULL) THEN 'string'").expect("the plain-string reading is present");
        assert!(last < fallback, "{sql}");
    }

    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`", which clause 4.23.2 ranks
    /// under Array, ahead of the Object every other structure is.
    #[test]
    fn an_array_or_a_list_ranks_as_an_array_rather_than_an_object() {
        let sql = rendered_token();
        let list_test = sql.find("? ('@list')").expect("the list keyword is tested");
        let structured_fallback = sql.find("\"o\" IS NOT NULL) THEN 'object'").expect("the structured-value reading is present");
        assert!(list_test < structured_fallback, "{sql}");
    }
}
