//! The clause 4.23.2 datatype of one expanded value node, and where that
//! datatype ranks.
//!
//! Clause 4.23.2: "When sorting by value, and the values of Attributes are of
//! mixed datatypes, the following comparison order (null values last), shall be
//! implicitly applied: Numbers, Strings, Object, Array, Boolean, Time, Date,
//! `DateTime`, Null" — and, last of all, "Attribute does not exist".
//!
//! Which datatype a value has is not a question `orderBy` gets to answer on its
//! own. Clause 4.6.3 names three data types whose whole representation *is* the
//! string — a `DateTime`, a `Date` and a `Time` — so a Property holding one
//! carries no type tag, and the broker reads it off the literal. That reading is
//! [`TimeValueForm`], the classifier the write projection and the clause 4.9 `q`
//! operand parser share, and the pattern below is that same classifier written
//! for a statement, which is why it is taken from the form rather than restated
//! here. A stored timestamp that `q` finds as a `DateTime` and this ranked under
//! Strings was one value with two datatypes.
//!
//! The order is rendered as a rank rather than as a chain of comparisons. The
//! datatype is read as a single token, and the token's position in a
//! nine-element sequence *is* its rank, so a descending sort is the same
//! expression over the reversed sequence and a datatype the payload does not
//! name ranks nowhere at all — which, sorted `NULLS LAST`, is exactly where
//! "attribute does not exist" belongs, in both directions.

use andromeda_ngsi_ld_lexicon::time_value::TimeValueForm;
use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
use andromeda_ngsi_ld_vocabulary::expanded::classes::{DATE, DATE_TIME, TIME};
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_key_exists, jsonb_typeof},
    literal::{int_literal, text_literal},
    operator::regex_match,
    pg_array::{array_position, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, ExprTrait, Func, SimpleExpr};

/// The JSON-LD keyword holding a value node's literal value.
pub(crate) const VALUE_MEMBER: &str = "@value";

/// The JSON-LD keyword holding a node's identifier.
pub(crate) const ID_MEMBER: &str = "@id";

/// The JSON-LD keyword holding a value node's datatype IRI.
const TYPE_MEMBER: &str = "@type";

/// The JSON-LD keyword holding the elements of an ordered list carrier.
///
/// Clauses 5.2.24 and 5.2.25 give the two list families an ordered value, which
/// JSON-LD expansion wraps in a single node under this keyword rather than
/// spreading over one node per element.
pub(crate) const LIST_MEMBER: &str = "@list";

/// The `jsonb_typeof` token for a JSON number.
pub(crate) const NUMBER_TYPE: &str = "number";

/// The `jsonb_typeof` token for a JSON string.
const STRING_TYPE: &str = "string";

/// The `jsonb_typeof` token for a JSON object.
const OBJECT_TYPE: &str = "object";

/// The `jsonb_typeof` token for a JSON array.
pub(crate) const ARRAY_TYPE: &str = "array";

/// The `jsonb_typeof` token for a JSON boolean.
const BOOLEAN_TYPE: &str = "boolean";

/// The `jsonb_typeof` token for a JSON null.
const NULL_TYPE: &str = "null";

/// The value-node count a scalar value has.
const SINGLE_VALUE_NODE: i64 = 1;

/// How many datatypes clause 4.23.2 ranks before "attribute does not exist".
const DATATYPE_COUNT: usize = 9;

/// How many readings of the payload can name a value's datatype.
const DATATYPE_SOURCE_COUNT: usize = 8;

/// The one token that names this value's clause-4.23.2 datatype.
///
/// Read in the order the payload distinguishes the datatypes in. More than one
/// value node means the attribute's value was a JSON array, whatever the nodes
/// hold, and so does a node carrying `@list`: clause 4.9 defines the target
/// value of a `ListProperty` as "the valueList array associated to such a
/// `ListProperty`" and that of a `ListRelationship` as "the array of objects"
/// it names, which clause 4.23.2 ranks under Array. Expansion wraps both in one
/// `@list` node, so their arity is 1 and only the keyword tells them apart from
/// a scalar.
///
/// A node carrying `@value` names its datatype through JSONB, except that a
/// string may be a `Time`, a `Date` or a `DateTime`. Those three are read the
/// way the rest of the broker reads them: a JSON-LD `@type` the client declared
/// decides first, and with none declared the clause 4.6.3 lexical form of the
/// literal does. A node carrying `@id` is a Relationship's target object, which
/// is a URI and compares as a string. Anything else that is a node at all is an
/// expanded structured value: an Object.
pub(crate) fn datatype_token(value_node: &SimpleExpr, arity: &SimpleExpr) -> SimpleExpr {
    let (time, date, date_time) = (TIME, DATE, DATE_TIME);
    // clone: sea-query owns every expression in its AST, so each of the readings
    // below takes its own copy of the two the lateral computed once.
    let from_arity = CaseStatement::new().case(arity.clone().ne(int_literal(SINGLE_VALUE_NODE)), text_literal(ARRAY_TYPE));
    let from_list = CaseStatement::new().case(jsonb_key_exists(value_node.clone(), text_literal(LIST_MEMBER)), text_literal(ARRAY_TYPE));
    let from_json_type = CaseStatement::new().case(
        jsonb_typeof(value_member(value_node)).ne(text_literal(STRING_TYPE)),
        jsonb_typeof(value_member(value_node)),
    );
    let from_ngsi_ld_type = CaseStatement::new().case(
        value_type(value_node).is_in([text_literal(time.as_str()), text_literal(date.as_str()), text_literal(date_time.as_str())]),
        value_type(value_node),
    );
    let from_lexical_form = CaseStatement::new()
        .case(written_in(value_node, TimeValueForm::DateTime), text_literal(date_time.as_str()))
        .case(written_in(value_node, TimeValueForm::Date), text_literal(date.as_str()))
        .case(written_in(value_node, TimeValueForm::Time), text_literal(time.as_str()));
    let from_string_value = CaseStatement::new().case(jsonb_key_exists(value_node.clone(), text_literal(VALUE_MEMBER)), text_literal(STRING_TYPE));
    let from_target_object = CaseStatement::new().case(jsonb_key_exists(value_node.clone(), text_literal(ID_MEMBER)), text_literal(STRING_TYPE));
    let from_structure = CaseStatement::new().case(value_node.clone().is_not_null(), text_literal(OBJECT_TYPE));
    let candidates: [SimpleExpr; DATATYPE_SOURCE_COUNT] = [
        from_arity.into(),
        from_list.into(),
        from_json_type.into(),
        from_ngsi_ld_type.into(),
        from_lexical_form.into(),
        from_string_value.into(),
        from_target_object.into(),
        from_structure.into(),
    ];
    Func::coalesce(candidates).into()
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

/// Whether the value node's literal is written in one clause 4.6.3 form.
///
/// The pattern is the lexical form's own, taken from the classifier the write
/// projection and the `q` operand parser read, so the three sides cannot come to
/// disagree about what a literal is.
fn written_in(value_node: &SimpleExpr, form: TimeValueForm) -> SimpleExpr {
    regex_match(value_text(value_node), text_literal(form.lexical_pattern()))
}

/// The value node's `@value` member, as JSONB.
fn value_member(value_node: &SimpleExpr) -> SimpleExpr {
    value_node.clone().json_get(text_literal(VALUE_MEMBER))
}

/// The value node's `@value` member, as text.
fn value_text(value_node: &SimpleExpr) -> SimpleExpr {
    value_node.clone().json_get_text(text_literal(VALUE_MEMBER))
}

/// The value node's `@type` member, as text: the NGSI-LD datatype IRI a
/// literal declares.
fn value_type(value_node: &SimpleExpr) -> SimpleExpr {
    value_node.clone().json_get_text(text_literal(TYPE_MEMBER))
}

#[cfg(test)]
mod tests {
    use crate::attribute_value_datatype::{datatype_rank, datatype_token};
    use andromeda_ngsi_ld_lexicon::time_value::TimeValueForm;
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn value_node() -> SimpleExpr {
        Expr::col(Alias::new("value_node"))
    }

    fn arity() -> SimpleExpr {
        Expr::col(Alias::new("value_arity"))
    }

    fn rendered_token() -> String {
        Query::select()
            .expr(datatype_token(&value_node(), &arity()))
            .to_owned()
            .to_string(PostgresQueryBuilder)
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

    /// Clause 4.6.3 names three data types whose whole representation is the
    /// string, so a Property holding one carries no `@type` and the literal is
    /// what says which it is. The pattern is the lexical form's own, which is
    /// what makes this the same answer the write projection and the clause 4.9
    /// operand parser give.
    #[test]
    fn a_literal_with_no_declared_type_is_classified_by_its_clause_4_6_3_form() {
        let sql = rendered_token();
        for pattern in [
            TimeValueForm::DateTime.lexical_pattern(),
            TimeValueForm::Date.lexical_pattern(),
            TimeValueForm::Time.lexical_pattern(),
        ] {
            assert!(sql.contains(pattern), "{pattern} is not read: {sql}");
        }
    }

    /// Annex C.6 lets a client declare the datatype explicitly, and clause 4.6.3
    /// makes the lexical form the reading when it did not, so the declaration is
    /// consulted first — exactly as the write projection consults it.
    #[test]
    fn a_declared_datatype_is_read_before_the_lexical_form() {
        let sql = rendered_token();
        let declared = sql.find("IN ('https://uri.etsi.org/ngsi-ld/Time'").expect("the declared reading is present");
        let lexical = sql.find(" ~ ").expect("the lexical reading is present");
        assert!(declared < lexical, "a declared datatype must win: {sql}");
    }

    /// A string that is written in none of the three forms is a String, so the
    /// lexical reading has to be tried before the plain-string fallback.
    #[test]
    fn the_lexical_reading_precedes_the_plain_string_fallback() {
        let sql = rendered_token();
        let lexical = sql.find(" ~ ").expect("the lexical reading is present");
        let fallback = sql.find("? ('@value')) THEN 'string'").expect("the plain-string reading is present");
        assert!(lexical < fallback, "{sql}");
    }

    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`" and a `ListRelationship`'s as
    /// "the array of objects" it names, which clause 4.23.2 ranks under Array.
    /// Expansion wraps both in one `@list` node, so the carrier's arity is 1 and
    /// only the keyword distinguishes it from a scalar — read as a bare node it
    /// fell through to Object.
    #[test]
    fn an_ordered_list_carrier_ranks_as_an_array_rather_than_an_object() {
        let sql = rendered_token();
        assert!(sql.contains("? ('@list')) THEN 'array'"), "{sql}");
        let list_test = sql.find("? ('@list')").expect("the list keyword is tested");
        let structured_fallback = sql.find("IS NOT NULL) THEN 'object'").expect("the structured-value reading is present");
        assert!(
            list_test < structured_fallback,
            "a list is ranked before anything falls through to Object: {sql}"
        );
    }
}
