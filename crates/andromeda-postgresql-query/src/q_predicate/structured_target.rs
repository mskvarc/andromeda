//! Comparing a clause 4.9 Query Term against a structured target value.
//!
//! # What is stored here
//!
//! `current_attribute_values` keeps one typed column per attribute instance,
//! and every attribute family whose value is not a single scalar lands in
//! `value_object` as the JSON the write path derived from the expanded
//! document:
//!
//! | Family (clause 5.2.4) | `value_object` |
//! |---|---|
//! | `LanguageProperty` | the expanded `languageMap`, `[{"@language": "en", "@value": "red"}, …]` |
//! | `ListProperty` | the `valueList`, `[{"@value": 1}, {"@value": 2}, …]` |
//! | `ListRelationship` | the `objectList`, `["urn:ngsi-ld:Part:1", …]` |
//! | `VocabProperty` carrying more than one term | the expanded `vocab`, `["http://example.org/Male", …]` |
//! | a Property whose value is an array or an object | the value itself |
//!
//! Clause 4.9 names each of those as a target value in its own right — "a value
//! from any of the key-value pairs held within the languageMap", "the valueList
//! array associated to such a `ListProperty`", "the array of objects associated
//! to such `ListRelationship`" — so a Query Term compared only against the scalar
//! columns would match none of them.
//!
//! # How the comparison is expressed
//!
//! JSONB containment, `@>`, *is* the clause 4.9 rule "Is included in the target
//! value, if the latter is an array", and it is strict about JSON datatypes,
//! which is the clause's other rule: "If there is no equality between the target
//! value data type and the Query Term value data type, then it shall be
//! considered as not matching", and, under `unequal`, "considered unequal".
//! PostgreSQL also treats a bare scalar as contained in an array, so one
//! operator serves both the elements that are value nodes and the elements that
//! are plain URIs.
//!
//! Two element shapes are therefore probed for a target with no language
//! selected — the bare element a `objectList`, a `vocab` list or an array
//! Property value carries, and the `{"@value": …}` node a `languageMap` or a
//! `valueList` carries — and one for a target language, which pins `@language`
//! as well.
//!
//! A temporal operand is compared as its canonical clause 4.9 text, because a
//! value inside a structure is stored as the client wrote it and JSON has no
//! instant type: an operand written with another offset does not match, in
//! keeping with the byte-wise comparison invariant the schema fixes elsewhere.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_build_array, jsonb_build_object},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// The JSON-LD keyword a value node carries its value under.
const VALUE_KEYWORD: &str = "@value";

/// The JSON-LD keyword a `languageMap` entry carries its language tag under.
const LANGUAGE_KEYWORD: &str = "@language";

/// The clause 4.9 target value a Query Term is compared against inside one
/// stored structured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredTarget<'a> {
    /// Every value the structure carries.
    ///
    /// Clause 4.9: with no target language specified, a `LanguageProperty`'s
    /// target value is "a value from any of the key-value pairs held within the
    /// languageMap"; a `ListProperty`'s is its whole `valueList`; a
    /// `ListRelationship`'s is its whole `objectList`.
    AnyValue,
    /// Only the values a `languageMap` carries under one target language.
    ///
    /// Clause 4.9: "the target value is defined as the Value associated to the
    /// matching key-value pair held within the languageMap associated to such
    /// `LanguageProperty` where the key matches the target language".
    TargetLanguage(&'a AttributeName),
}

/// Whether the structured value in `column` carries `scalar` as its clause 4.9
/// target value.
///
/// This is the `equal` operator over a structured target: the Query Term value
/// "is identical or equivalent to the target value" or "is included in the
/// target value, if the latter is an array", including the language-map form
/// where a match may be "a single element from the array of values of the
/// key-value pair".
#[must_use]
pub fn carries_operand(column: &SimpleExpr, target: StructuredTarget<'_>, scalar: &Scalar) -> Condition {
    match target {
        StructuredTarget::AnyValue => Condition::any()
            .add(contains(column, bare_element(scalar)))
            .add(contains(column, value_element(scalar))),
        StructuredTarget::TargetLanguage(language) => Condition::all().add(contains(column, tagged_element(language, scalar))),
    }
}

/// Whether the structured value in `column` holds a `languageMap` entry under
/// one target language.
///
/// Clause 4.9 requires a matching Entity to "contain the target element", and
/// with a target language specified the target element is the matching
/// key-value pair — so this is what a bare existence Query Term asks of a
/// language-selected path.
#[must_use]
pub fn carries_target_language(column: &SimpleExpr, language: &AttributeName) -> Condition {
    let element = jsonb_build_array(jsonb_build_object([text_literal(LANGUAGE_KEYWORD), language_operand(language)]));
    Condition::all().add(contains(column, element))
}

/// `column @> element`, the containment test every comparison here is built on.
fn contains(column: &SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    // clone: sea-query owns every operand, and one column is read by several tests
    column.clone().array_contains(element)
}

/// The one-element array holding the operand as a bare JSON value, which is the
/// element shape an `objectList`, a multi-term `vocab` and an array Property
/// value carry.
fn bare_element(scalar: &Scalar) -> SimpleExpr {
    jsonb_build_array(operand(scalar))
}

/// The one-element array holding the operand as an expanded value node, which is
/// the element shape a `languageMap` and a `valueList` carry.
fn value_element(scalar: &Scalar) -> SimpleExpr {
    jsonb_build_array(jsonb_build_object([text_literal(VALUE_KEYWORD), operand(scalar)]))
}

/// The one-element array holding the operand as a `languageMap` entry under one
/// target language.
///
/// Containment matches an entry that carries these two members whatever else it
/// carries, so an entry written with any further JSON-LD member still matches,
/// and an entry under another language never does.
fn tagged_element(language: &AttributeName, scalar: &Scalar) -> SimpleExpr {
    jsonb_build_array(jsonb_build_object([
        text_literal(LANGUAGE_KEYWORD),
        language_operand(language),
        text_literal(VALUE_KEYWORD),
        operand(scalar),
    ]))
}

/// The target language, bound as text.
///
/// It reaches the statement as a parameter rather than inlined, because it is
/// the client's own text: only the two JSON-LD keywords are written into the
/// statement itself.
fn language_operand(language: &AttributeName) -> SimpleExpr {
    cast(SeaExpr::val(language.as_str()), PostgresType::Text)
}

/// The Query Term operand, bound as the PostgreSQL type its JSON counterpart is
/// built from.
///
/// Every operand carries an explicit cast: `jsonb_build_array` and
/// `jsonb_build_object` are `VARIADIC "any"`, and an untyped placeholder in that
/// position has no type for the server to resolve.
///
/// A number binds as `NUMERIC`, so an integer beyond 2^53 and an exact fraction
/// reach the comparison as themselves and JSONB's own numeric equality decides
/// it — `2` and `2.0` are one number there, as they are in the `value_number`
/// column. The three temporal kinds bind as their canonical clause 4.9 text,
/// which is the only form a JSON string can be compared in.
fn operand(scalar: &Scalar) -> SimpleExpr {
    match scalar {
        // clone: sea-query owns every value in its AST, so the exact decimal is
        // cloned out of the borrowed query operand exactly as the typed column
        // comparison clones it.
        Scalar::Number(number) => cast(SeaExpr::val(number.decimal().clone()), PostgresType::Numeric),
        Scalar::Bool(value) => cast(SeaExpr::val(*value), PostgresType::Boolean),
        Scalar::Str(value) | Scalar::Term(value) | Scalar::Uri(value) => cast(SeaExpr::val(value.as_str()), PostgresType::Text),
        Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => cast(SeaExpr::val(scalar.literal_text()), PostgresType::Text),
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::structured_target::{StructuredTarget, carries_operand, carries_target_language};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::filter::scalar::{Scalar, parse_date_time};
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, Condition, Expr as SeaExpr, PostgresQueryBuilder, Query, SimpleExpr, Value};

    /// A stand-in for the probe's `"cav"."value_object"` reference.
    fn column() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_object"))
    }

    fn language(tag: &str) -> AttributeName {
        tag.parse().expect("a valid trailing member")
    }

    /// Render one condition the way a probe would, so the assertions see the
    /// placeholder numbering the operands actually get.
    fn render(condition: Condition) -> (String, Vec<Value>) {
        let (sql, values) = Query::select().expr(int_literal(1)).cond_where(condition).build(PostgresQueryBuilder);
        (sql, values.0)
    }

    #[test]
    fn an_unselected_target_probes_both_element_shapes() {
        // Clause 4.9 names an `objectList` of bare URIs and a `valueList` of
        // value nodes as targets of the same Query Term, so one comparison has
        // to reach both element shapes.
        let (sql, _) = render(carries_operand(&column(), StructuredTarget::AnyValue, &Scalar::Str("red".to_owned())));
        assert!(sql.contains("\"value_object\" @> jsonb_build_array($1::text)"), "{sql}");
        assert!(
            sql.contains("\"value_object\" @> jsonb_build_array(jsonb_build_object('@value', $2::text))"),
            "{sql}"
        );
        assert!(sql.contains(" OR "), "either shape matches: {sql}");
    }

    #[test]
    fn a_target_language_pins_the_language_of_the_entry_it_matches() {
        // Clause 4.9: with a target language specified the target value is the
        // value of the key-value pair whose key matches it, so no other key's
        // value may satisfy the term.
        let (sql, binds) = render(carries_operand(
            &column(),
            StructuredTarget::TargetLanguage(&language("en")),
            &Scalar::Str("red".to_owned()),
        ));
        assert!(
            sql.contains("jsonb_build_object('@language', $1::text, '@value', $2::text)"),
            "the language is pinned beside the value: {sql}"
        );
        assert!(
            matches!(&binds[0], Value::String(Some(text)) if text.as_str() == "en"),
            "the target language binds first: {binds:?}"
        );
        assert!(matches!(&binds[1], Value::String(Some(text)) if text.as_str() == "red"), "{binds:?}");
    }

    #[test]
    fn a_bare_existence_of_a_target_language_tests_the_key_alone() {
        let (sql, binds) = render(carries_target_language(&column(), &language("de")));
        assert!(sql.contains("jsonb_build_array(jsonb_build_object('@language', $1::text))"), "{sql}");
        assert!(!sql.contains("@value"), "existence says nothing about the value: {sql}");
        assert!(matches!(&binds[0], Value::String(Some(text)) if text.as_str() == "de"), "{binds:?}");
    }

    #[test]
    fn a_number_operand_binds_numeric_so_json_compares_it_as_a_number() {
        // A `valueList` of numbers must not be matched by the string spelling of
        // one: clause 4.9 makes a datatype difference a non-match, and JSONB
        // containment enforces exactly that once the operand is a JSON number.
        let (sql, binds) = render(carries_operand(
            &column(),
            StructuredTarget::AnyValue,
            &Scalar::Number(JsonNumber::from(9_007_199_254_740_993_i64)),
        ));
        assert!(sql.contains("$1::numeric"), "{sql}");
        assert!(
            matches!(&binds[0], Value::BigDecimal(Some(value)) if value.to_string() == "9007199254740993"),
            "{binds:?}"
        );
    }

    #[test]
    fn a_boolean_operand_binds_a_native_boolean() {
        let (sql, binds) = render(carries_operand(&column(), StructuredTarget::AnyValue, &Scalar::Bool(true)));
        assert!(sql.contains("$1::boolean"), "{sql}");
        assert!(binds.iter().any(|bind| matches!(bind, Value::Bool(Some(true)))), "{binds:?}");
    }

    #[test]
    fn a_temporal_operand_binds_its_canonical_clause_4_9_text() {
        // A value inside a structure is stored as the client wrote it, and JSON
        // carries no instant type, so the comparison is against the one
        // canonical rendering the query language defines.
        let instant = parse_date_time("2024-01-01T00:00:00,000Z").expect("valid date-time");
        let (sql, binds) = render(carries_operand(&column(), StructuredTarget::AnyValue, &Scalar::DateTime(instant)));
        assert!(sql.contains("$1::text"), "{sql}");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "2024-01-01T00:00:00Z")),
            "{binds:?}"
        );
    }
}
