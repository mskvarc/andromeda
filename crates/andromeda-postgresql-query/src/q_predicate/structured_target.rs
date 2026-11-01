//! Comparing a clause 4.9 Query Term against a structured target value.
//!
//! # What is stored here
//!
//! A stored instance keeps one typed field per scalar kind,
//! and every attribute family whose value is not a single scalar lands in
//! `value_object` as the JSON the write path derived from the expanded
//! document:
//!
//! | Family (clause 5.2.4) | `value_object` |
//! |---|---|
//! | `LanguageProperty` | the expanded `languageMap`, `[{"@language": "en", "@value": "red"}, …]` |
//! | `ListProperty` | the `valueList`, `[{"@value": 1}, {"@value": 2}, …]` |
//! | `ListRelationship` | the `objectList`, `["urn:ngsi-ld:Part:1", …]` |
//! | Relationship holding more than one object | the objects, `["urn:ngsi-ld:Part:1", …]` |
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
//! JSONB containment, `@>`, is the clause 4.9 rule "Is included in the target
//! value, if the latter is an array", and PostgreSQL treats a bare scalar as
//! contained in an array, so one operator serves both the elements that are
//! value nodes and the elements that are plain URIs. Two element shapes are
//! probed: the bare element a `objectList`, a `vocab` list or an array
//! Property value carries, and the `{"@value": …}` node a `languageMap` or a
//! `valueList` carries. A target language is no structured target here:
//! containment compares `@language` byte for byte, while RFC 5646 section
//! 2.1.1 compares tags case-insensitively, so a language-selected path is
//! decided over the map's entries instead.
//!
//! Containment compares JSON types, and clause 4.9 compares data types: "If
//! there is no equality between the target value data type and the Query Term
//! value data type, then it shall be considered as not matching", and, under
//! `unequal`, "considered unequal". A member's data type is its declared
//! datatype, the `valueType` of a Property or a `ListProperty` before the
//! member's own `@type`, so a JSON string may be a number, a Boolean or a time
//! value. A `JsonProperty` holds one JSON literal, which equals no scalar
//! operand, so it carries none. A number or a
//! Boolean operand is therefore probed as its JSON value and as each JSON
//! string a member declared as one is committed in (its canonical lexical
//! forms), and a String operand as its text; [`some_member_equals`] then
//! reads the members of each candidate under their declarations and decides
//! it. Every probe stays a containment test the GIN index on `value_object`
//! answers.
//!
//! A `DateTime`, `Date` or `Time` operand matches only a member declared as
//! a value of its form. A declared member is committed in its one served
//! spelling, so the operand is contained once: as a value node carrying the
//! form's `@type`, or as the plain text when the instance's `valueType` names
//! the form (annex C.6), and needs no reading of the members.

use crate::q_predicate::declared_member::some_member_equals;
use andromeda_ngsi_ld_lexicon::{
    datatype::declared_value::NumberSpelling,
    time::{
        date::NgsiLdDate,
        rendering::{instant_text, time_of_day_text},
        value::TimeValueForm,
    },
};
use andromeda_ngsi_ld_number::declared_number::DeclaredNumber;
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_ngsi_ld_vocabulary::keyword;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::name_dictionary::EntityTypeNames;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_build_array, jsonb_build_object},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Query, QueryStatementBuilder, SimpleExpr};

/// Whether the structured value in `column` carries `scalar` as its clause 4.9
/// target value, `value_type` being the instance's `valueType` key.
///
/// This is the `equal` operator over a structured target with no target
/// language: the Query Term value "is identical or equivalent to the target
/// value" or "is included in the target value, if the latter is an array". With
/// no target language specified, a `LanguageProperty`'s target value is "a
/// value from any of the key-value pairs held within the languageMap"; a
/// `ListProperty`'s is its whole `valueList`; a `ListRelationship`'s is its
/// whole `objectList`.
///
/// `family` is the instance's attribute family code. A `JsonProperty` is no
/// structured target: its value is one JSON literal, an object or an array of
/// objects, which equals no scalar operand, and a member it holds under
/// `@value` is JSON, no value node. Only a Property's and a `ListProperty`'s
/// `valueType` declares their members (clause 4.5.2).
#[must_use]
pub fn carries_operand(column: &SimpleExpr, value_type: &SimpleExpr, family: &SimpleExpr, scalar: &Scalar) -> Condition {
    let declaring = declaring_value_type(value_type, family);
    Condition::all()
        // clone: sea-query owns every operand, and one column is read by several tests
        .add(family.clone().ne(int_literal(i64::from(AttributeKind::JsonProperty.code()))))
        .add(carried(column, &declaring, scalar))
}

/// The `valueType` key that declares the members of the instance of the
/// attribute family `family`: its own `value_type` for a Property or a
/// `ListProperty`, NULL for every other family.
fn declaring_value_type(value_type: &SimpleExpr, family: &SimpleExpr) -> SimpleExpr {
    let declaring = [AttributeKind::Property, AttributeKind::ListProperty].map(|kind| int_literal(i64::from(kind.code())));
    // clone: sea-query owns every operand, and one column is read by several tests
    CaseStatement::new().case(family.clone().is_in(declaring), value_type.clone()).into()
}

/// [`carries_operand`] once the instance's family holds a structured target,
/// `value_type` being the `valueType` key that declares its members.
fn carried(column: &SimpleExpr, value_type: &SimpleExpr, scalar: &Scalar) -> Condition {
    match member_text(scalar) {
        MemberText::Read { native, spellings } => {
            let probes = native
                .into_iter()
                .chain(spellings.into_iter().map(|text| cast(SeaExpr::val(text), PostgresType::Text)));
            let candidate = probes.fold(Condition::any(), |candidate, operand| {
                candidate
                    // clone: each element shape is built around its own copy of the operand
                    .add(contains(column, bare_element(operand.clone())))
                    .add(contains(column, value_element(operand)))
            });
            Condition::all().add(candidate).add(some_member_equals(column, value_type, scalar))
        }
        MemberText::Declared { form, text } => {
            let plain = Condition::any()
                .add(contains(column, json_element(&format!("[\"{text}\"]"))))
                .add(contains(column, json_element(&format!("[{{\"{}\": \"{text}\"}}]", keyword::VALUE))));
            Condition::any()
                .add(contains(
                    column,
                    json_element(&format!(
                        "[{{\"{}\": \"{text}\", \"{}\": \"{}\"}}]",
                        keyword::VALUE,
                        keyword::TYPE,
                        form.iri().as_str()
                    )),
                ))
                .add(Condition::all().add(value_type_names(value_type, form)).add(plain))
        }
    }
}

/// What a member equal to a Query Term operand holds.
enum MemberText {
    /// An operand whose members are read under their declarations to decide
    /// it ([`some_member_equals`]).
    Read {
        /// The JSON number or boolean a member equal to the operand holds,
        /// bound as the PostgreSQL type its JSON counterpart is built from;
        /// none for a String operand.
        native: Option<SimpleExpr>,
        /// The JSON strings a member equal to the operand may be: a String
        /// operand's text, the canonical lexical forms of a number, the
        /// spelling of a Boolean.
        spellings: Vec<String>,
    },
    /// The time value a member declared as one of `form` holds, as `text`,
    /// its one committed spelling.
    Declared {
        /// The time value form the operand is written in.
        form: TimeValueForm,
        /// The operand's served spelling.
        text: String,
    },
}

/// One single-element array a member is contained as, bound as `jsonb`.
///
/// The element is written as JSON text rather than built in the statement:
/// `jsonb_build_array` is `STABLE`, so a probe that reads every row would build
/// the element again for every row, while the `jsonb` input function is
/// `IMMUTABLE` and the planner reads the bound text once. A served time value
/// is digits, `-`, `:`, `T`, `.` and `Z`, and a form's IRI a fixed URL, none
/// of which a JSON string escapes.
fn json_element(json: &str) -> SimpleExpr {
    cast(SeaExpr::val(json), PostgresType::Jsonb)
}

/// Whether the instance's `valueType` key `value_type` is the dictionary key
/// of `form`'s datatype IRI.
fn value_type_names(value_type: &SimpleExpr, form: TimeValueForm) -> SimpleExpr {
    let key = Query::select()
        .column(EntityTypeNames::EntityTypeNameKey)
        .from(EntityTypeNames::Table)
        .and_where(SeaExpr::col(EntityTypeNames::Name).eq(form.iri().as_str()))
        .to_owned();
    // clone: sea-query owns every operand, and one column is read by several tests
    value_type.clone().eq(SimpleExpr::SubQuery(None, Box::new(key.into_sub_query_statement())))
}

/// `column @> element`, the containment test every comparison here is built on.
fn contains(column: &SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    // clone: sea-query owns every operand, and one column is read by several tests
    column.clone().array_contains(element)
}

/// The one-element array holding the operand as a bare JSON value, which is the
/// element shape an `objectList`, a multi-term `vocab` and an array Property
/// value carry.
fn bare_element(operand: SimpleExpr) -> SimpleExpr {
    jsonb_build_array(operand)
}

/// The one-element array holding the operand as an expanded value node, which is
/// the element shape a `languageMap` and a `valueList` carry.
fn value_element(operand: SimpleExpr) -> SimpleExpr {
    jsonb_build_array(jsonb_build_object([text_literal(keyword::VALUE), operand]))
}

/// What a member equal to `scalar` holds.
///
/// Every operand carries an explicit cast: `jsonb_build_array` and
/// `jsonb_build_object` are `VARIADIC "any"`, and an untyped placeholder in that
/// position has no type for the server to resolve.
///
/// A number binds as `NUMERIC`, so an integer beyond 2^53 and an exact fraction
/// reach the comparison as themselves and JSONB's own numeric equality decides
/// it: `2` and `2.0` are one number there, as they are in the `value_number`
/// column. A declared number is committed in the canonical lexical form of its
/// datatype ([`NumberSpelling`]), of which there are two, often alike.
fn member_text(scalar: &Scalar) -> MemberText {
    match scalar {
        Scalar::Number(number) => {
            // clone: the declared number the spellings are built from owns its value
            let declared = DeclaredNumber::Finite(number.clone());
            let mut spellings = vec![NumberSpelling::Decimal.spell(&declared)];
            let floating = NumberSpelling::Floating.spell(&declared);
            if !spellings.contains(&floating) {
                spellings.push(floating);
            }
            MemberText::Read {
                // clone: sea-query owns every value in its AST, so the exact
                // decimal is cloned out of the borrowed query operand exactly as
                // the typed column comparison clones it.
                native: Some(cast(SeaExpr::val(number.decimal().clone()), PostgresType::Numeric)),
                spellings,
            }
        }
        Scalar::Bool(value) => MemberText::Read {
            native: Some(cast(SeaExpr::val(*value), PostgresType::Boolean)),
            spellings: vec![value.to_string()],
        },
        Scalar::Str(value) | Scalar::Uri(value) => MemberText::Read {
            native: None,
            spellings: vec![value.as_str().to_owned()],
        },
        Scalar::DateTime(instant) => MemberText::Declared {
            form: TimeValueForm::DateTime,
            text: instant_text(*instant),
        },
        Scalar::Date(date) => MemberText::Declared {
            form: TimeValueForm::Date,
            text: NgsiLdDate::from(*date).to_string(),
        },
        Scalar::Time(time) => MemberText::Declared {
            form: TimeValueForm::Time,
            text: time_of_day_text(*time),
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::q_predicate::structured_target::carries_operand;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::filter::scalar::{Scalar, parse_date, parse_date_time, parse_time};
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, Condition, Expr as SeaExpr, PostgresQueryBuilder, Query, SimpleExpr, Value};

    /// A stand-in for the probe's `"ca"."value_object"` reference.
    fn column() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_object"))
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
        let (sql, _) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Str("red".to_owned())));
        assert!(sql.contains("\"value_object\" @> jsonb_build_array($1::text)"), "{sql}");
        assert!(
            sql.contains("\"value_object\" @> jsonb_build_array(jsonb_build_object('@value', $2::text))"),
            "{sql}"
        );
        assert!(sql.contains(" OR "), "either shape matches: {sql}");
    }

    #[test]
    fn a_number_operand_binds_numeric_so_json_compares_it_as_a_number() {
        // A `valueList` of numbers must not be matched by the string spelling of
        // one: clause 4.9 makes a datatype difference a non-match, and JSONB
        // containment enforces exactly that once the operand is a JSON number.
        let (sql, binds) = render(carries_operand(
            &column(),
            &value_type(),
            &family(),
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
        let (sql, binds) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Bool(true)));
        assert!(sql.contains("$1::boolean"), "{sql}");
        assert!(binds.iter().any(|bind| matches!(bind, Value::Bool(Some(true)))), "{binds:?}");
    }

    /// A declared number is committed as a JSON string in the canonical
    /// lexical form of its datatype, so the operand is also probed as each
    /// form, and the members of a candidate decide it.
    #[test]
    fn a_number_operand_is_probed_as_each_canonical_spelling_and_read_back() {
        let number = "1E-7".parse::<JsonNumber>().expect("a JSON number");
        let (sql, binds) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Number(number)));
        let texts: Vec<&str> = binds
            .iter()
            .filter_map(|bind| if let Value::String(Some(text)) = bind { Some(text.as_str()) } else { None })
            .collect();
        assert!(texts.contains(&"0.0000001") && texts.contains(&"1E-7"), "{binds:?}");
        pretty_assertions::assert_eq!(sql.matches("\"value_object\" @> ").count(), 6, "{sql}");
        assert!(sql.contains("jsonb_array_elements"), "{sql}");
    }

    #[test]
    fn a_number_spelled_alike_in_both_forms_is_probed_once_as_text() {
        let (sql, _) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Number(JsonNumber::from(4))));
        pretty_assertions::assert_eq!(sql.matches("\"value_object\" @> ").count(), 4, "{sql}");
    }

    /// Clause 4.9: a JSON string declared as another data type is no String,
    /// so containment of the text only narrows the candidates.
    #[test]
    fn a_string_operand_reads_the_members_of_a_candidate_under_their_declarations() {
        let (sql, _) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Str("4".to_owned())));
        assert!(sql.contains(") AND EXISTS(SELECT 1 FROM jsonb_array_elements"), "{sql}");
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#integer'"), "{sql}");
    }

    /// The column naming the datatype the row's `valueType` declares.
    fn value_type() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_type_key"))
    }

    /// The column naming the row's attribute family.
    fn family() -> SimpleExpr {
        SeaExpr::col(Alias::new("attribute_kind"))
    }

    /// A `JsonProperty` holds one JSON literal, which equals no scalar
    /// operand.
    #[test]
    fn a_json_property_carries_no_operand() {
        let (sql, _) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Number(JsonNumber::from(4))));
        assert!(sql.starts_with(r#"SELECT 1 WHERE "attribute_kind" <> (6) AND "#), "{sql}");
    }

    /// Clause 4.5.2: the `valueType` of a Property or a `ListProperty`
    /// declares their members; no other family's does.
    #[test]
    fn only_a_property_or_a_list_property_value_type_declares_the_members() {
        let (sql, _) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Number(JsonNumber::from(4))));
        assert!(
            sql.contains(r#"ON "dmt"."entity_type_name_key" = (CASE WHEN ("attribute_kind" IN (1, 7)) THEN "value_type_key" END)"#),
            "{sql}"
        );
    }

    fn json_binds(binds: &[Value]) -> Vec<&str> {
        binds
            .iter()
            .filter_map(|bind| if let Value::String(Some(text)) = bind { Some(text.as_str()) } else { None })
            .collect()
    }

    /// A declared member is committed in its one served spelling, so the
    /// operand is contained once as a typed node, or once in each plain shape
    /// under a `valueType` naming its form.
    #[test]
    fn a_date_time_operand_is_contained_once_as_a_declared_member() {
        let instant = parse_date_time("2024-01-01T00:00:00.000Z").expect("valid date-time");
        let (sql, binds) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::DateTime(instant)));
        pretty_assertions::assert_eq!(sql.matches("\"value_object\" @> ($").count(), 3, "{sql}");
        assert!(
            sql.contains(r#"THEN "value_type_key" END) = (SELECT "entity_type_name_key" FROM "entity_type_names""#),
            "{sql}"
        );
        pretty_assertions::assert_eq!(
            json_binds(&binds),
            vec![
                r#"[{"@value": "2024-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]"#,
                "https://uri.etsi.org/ngsi-ld/DateTime",
                r#"["2024-01-01T00:00:00Z"]"#,
                r#"[{"@value": "2024-01-01T00:00:00Z"}]"#,
            ]
        );
    }

    #[test]
    fn a_time_operand_is_contained_in_its_served_spelling() {
        let time = parse_time("12:00:00,5Z").expect("valid time");
        let (_, binds) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Time(time)));
        assert!(
            json_binds(&binds).contains(&r#"[{"@value": "12:00:00.500Z", "@type": "https://uri.etsi.org/ngsi-ld/Time"}]"#),
            "{binds:?}"
        );
    }

    #[test]
    fn a_date_operand_is_contained_as_its_one_text() {
        let date = parse_date("2024-01-01").expect("valid date");
        let (_, binds) = render(carries_operand(&column(), &value_type(), &family(), &Scalar::Date(date)));
        assert!(json_binds(&binds).contains(&r#"["2024-01-01"]"#), "{binds:?}");
    }
}
