//! The index path of a sub-attribute equality: the rows whose sub-attribute
//! tree holds the operand at the term's name path, read through
//! `idx_ca_sub_eq_keys`.
//!
//! The index keys the expression `sub_eq_keys(attribute_name_key,
//! sub_attributes)`, one key per scalar value of an element, and the planner
//! matches an expression index only on an identical expression, so
//! [`key_expression`] renders it character for character. A key selects
//! candidates only, so every candidate is rechecked by the same element
//! condition the term compiles to.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::equality_value,
        instance_row::ProbedRow,
        language_map::LanguageSelection,
        local_target::{LocalTarget, local_target},
        projection_probe::{Ca, of_attribute, projection_column, standing},
        structured_target::StructuredTarget,
        sub_attribute::element_path::ElementPath,
    },
};
use andromeda_ngsi_ld_lexicon::time::{
    date::NgsiLdDate,
    spellings::{instant_spellings, time_of_day_spellings},
    time_of_day::NgsiLdTime,
};
use andromeda_ngsi_ld_query::filter::{
    path::QueryPath,
    predicate::{Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    cast::cast,
    operator::key_array,
    pg_array::array_cat,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    sub_attribute_key::{sub_eq_keys, sub_eq_query_key, sub_eq_query_text_key},
};
use sea_query::{Expr as SeaExpr, Query, SelectStatement, SimpleExpr};

/// The `value_kind` salt of a String key.
const STRING_KIND: i16 = 2;

/// The `value_kind` salt of a URI key.
const URI_KIND: i16 = 7;

/// `sub_eq_keys("ca"."attribute_name_key", "ca"."sub_attributes")`, the
/// expression `idx_ca_sub_eq_keys` indexes.
#[must_use]
pub fn key_expression() -> SimpleExpr {
    sub_eq_keys(
        projection_column(CurrentAttributes::AttributeNameKey),
        projection_column(CurrentAttributes::SubAttributes),
    )
}

/// `SELECT "ca"."entity_key"` over the rows whose sub-attribute tree holds the
/// operand of `pred` at its name path, `None` unless the key index answers
/// `pred`: `==` with one operand on a sub-attribute's own value, every name of
/// its path recorded.
pub(crate) fn equality_candidates(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    let Predicate::Equality {
        path: QueryPath::Local(path),
        membership: SetMembership::Within,
        rhs: rhs @ CompareValue::Scalar(scalar),
    } = pred
    else {
        return None;
    };
    match local_target(path) {
        LocalTarget::SubAttribute(Some(LanguageSelection::AnyLanguage)) => {}
        LocalTarget::OwnValue
        | LocalTarget::TrailingMember(_)
        | LocalTarget::NestedMember
        | LocalTarget::SubAttribute(Some(LanguageSelection::TargetLanguage(_)) | None) => return None,
    }
    let element = ElementPath::resolve(path, ProbedRow::Current, &names.attributes)?;
    if element.chain.is_empty() || element.field.is_some() {
        return None;
    }
    let keys = query_keys(&element.key_path(), scalar);
    let value = equality_value(StructuredTarget::AnyValue, SetMembership::Within, rhs, element.holder());
    let mut select = Query::select();
    select
        .expr(projection_column(CurrentAttributes::EntityKey))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(element.attribute))
        .and_where(standing())
        .and_where(key_expression().array_overlaps(keys))
        .cond_where(element.condition(value));
    Some(select.take())
}

/// The keys `scalar` has at `path`: every key a stored value equal to it can
/// have.
///
/// A String or a URI operand may equal a String or a Relationship object, and
/// the two are salted apart, so both keys are read. A `Date` is held as its
/// one text. A `DateTime` is held as an instant in a scalar element and as the
/// text its client wrote inside an array, and a `Time` as the text its client
/// wrote everywhere, so each reads the key of every spelling clause 4.6.3
/// admits for its value, and a `DateTime` its instant key too. The fixed-width
/// spelling of a `Time` leads, the key a scalar element is also keyed by.
fn query_keys(path: &[i32], scalar: &Scalar) -> SimpleExpr {
    let path = || key_array(path);
    let key = |value: SimpleExpr| sub_eq_query_key(path(), value);
    let text_key = |kind: i16, text: String| sub_eq_query_text_key(path(), kind, text);
    match scalar {
        // clone: sea-query owns every value in its AST
        Scalar::Number(number) => key(cast(SeaExpr::val(number.decimal().clone()), PostgresType::Numeric)),
        Scalar::Bool(value) => key(cast(SeaExpr::val(*value), PostgresType::Boolean)),
        Scalar::Str(value) | Scalar::Uri(value) => array_cat(text_key(STRING_KIND, value.to_owned()), text_key(URI_KIND, value.to_owned())),
        Scalar::DateTime(value) => instant_spellings(*value)
            .into_iter()
            .fold(key(cast(SeaExpr::val(*value), PostgresType::Timestamptz)), |keys, spelling| {
                array_cat(keys, text_key(STRING_KIND, spelling))
            }),
        Scalar::Date(value) => text_key(STRING_KIND, NgsiLdDate::from(*value).to_string()),
        Scalar::Time(value) => {
            let fixed_width = NgsiLdTime::normalise(*value).to_string();
            time_of_day_spellings(*value)
                .into_iter()
                .filter(|spelling| *spelling != fixed_width)
                // clone: the leading key owns its text while the filter compares against it
                .fold(text_key(STRING_KIND, fixed_width.clone()), |keys, spelling| {
                    array_cat(keys, text_key(STRING_KIND, spelling))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::{keyed_term::keyed_candidates, sub_attribute::keyed_equality::key_expression},
        temporal_operand::STORED_TIME_TEMPLATE,
    };
    use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
    use andromeda_ngsi_ld_query::filter::parser::parse;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    /// The `current_attributes` migration, which declares `idx_ca_sub_eq_keys`.
    const MIGRATION: &str = include_str!("../../../../../migrations/branch/0022_current_attributes.sql");

    /// The functions migration, which defines `sub_eq_keys`.
    const FUNCTIONS: &str = include_str!("../../../../../migrations/tenant/0008_functions.sql");

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 2), ("accuracy", 5), ("source", 7)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
        }
    }

    fn candidates(q: &str) -> Option<String> {
        let expr = parse(q).expect("parse q");
        keyed_candidates(&expr, &EntityRowRefs::unaliased(None), &names()).map(|keyed| keyed.select.to_string(PostgresQueryBuilder))
    }

    /// The text between the parentheses after `using gin` in the index
    /// declaration, unqualified and unquoted as the migration writes it.
    fn indexed_expression() -> String {
        let declaration = MIGRATION
            .split("create index idx_ca_sub_eq_keys")
            .nth(1)
            .expect("the migration declares the index");
        let opened = declaration.split_once("using gin (").expect("a GIN index").1;
        let mut depth = 1;
        let end = opened
            .char_indices()
            .find_map(|(position, character)| {
                match character {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _other => {}
                }
                (depth == 0).then_some(position)
            })
            .expect("the expression closes");
        opened[..end].to_owned()
    }

    /// Invariant 9: the planner matches an expression index only on an
    /// identical expression.
    #[test]
    fn the_compiled_key_expression_matches_the_index() {
        let rendered = Query::select().expr(key_expression()).to_owned().to_string(PostgresQueryBuilder);
        let rendered = rendered.trim_start_matches("SELECT ").replace("\"ca\".", "").replace('"', "");
        assert_eq!(rendered, indexed_expression());
    }

    #[test]
    fn a_keyed_equality_reads_its_candidates_through_the_index() {
        let sql = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"").expect("keyed");
        assert!(
            sql.contains(r#"sub_eq_keys("ca"."attribute_name_key", "ca"."sub_attributes") && array_cat(sub_eq_query_key(ARRAY[2, 7]::integer[], 2::smallint, 'sensor-43'::text), sub_eq_query_key(ARRAY[2, 7]::integer[], 7::smallint, 'sensor-43'::text))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM UNNEST("ca"."sub_attributes") AS "l0""#),
            "the candidates are rechecked: {sql}"
        );
    }

    /// The text key of one spelling at the `temperature.source` path.
    fn spelling_key(spelling: &str) -> String {
        format!("sub_eq_query_key(ARRAY[2, 7]::integer[], 2::smallint, '{spelling}'::text)")
    }

    /// A member keeps its written spelling, so a `Time` reads the key of every
    /// spelling clause 4.6.3 admits, whichever one the operand is written in.
    #[test]
    fn a_time_equality_reads_the_key_of_every_spelling() {
        for operand in ["12:00:00Z", "12:00:00.000Z"] {
            let sql = candidates(&format!("\"https://example.com/temperature\".\"https://example.com/source\"=={operand}")).expect("keyed");
            for spelling in ["12:00:00.000000Z", "12:00:00Z", "12:00:00.0Z", "12:00:00,000Z", "12:00:00,000000Z"] {
                assert!(sql.contains(&spelling_key(spelling)), "{operand} reads {spelling}: {sql}");
            }
            pretty_assertions::assert_eq!(sql.matches("2::smallint").count(), 13, "{operand}: {sql}");
        }
    }

    /// A scalar `DateTime` element is keyed by its instant and an array member
    /// by its written text, so the operand reads both.
    #[test]
    fn a_date_time_equality_reads_the_instant_key_and_the_key_of_every_spelling() {
        let sql = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==2024-01-01T00:00:00.000Z").expect("keyed");
        assert!(
            sql.contains("sub_eq_query_key(ARRAY[2, 7]::integer[], '2024-01-01 00:00:00.000000 +00:00'::timestamptz)"),
            "{sql}"
        );
        for spelling in ["2024-01-01T00:00:00Z", "2024-01-01T00:00:00.000Z", "2024-01-01T00:00:00,000000Z"] {
            assert!(sql.contains(&spelling_key(spelling)), "{spelling}: {sql}");
        }
    }

    #[test]
    fn only_a_sub_attribute_equality_with_one_operand_is_keyed() {
        for q in [
            "\"https://example.com/temperature\".\"https://example.com/accuracy\">0.5",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"!=0.5",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5,0.6",
            "\"https://example.com/temperature\"==0.5",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5|\"https://example.com/temperature\"==1",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
        assert!(candidates("\"https://example.com/temperature\"==1;\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5").is_some());
    }

    /// The index keys a stored time of day by the same reading of clause 4.6.3
    /// a `q` operand and the JSONB lift use: the lexicon's pattern of the form,
    /// the comma read as the decimal point, and the fixed-width spelling.
    #[test]
    fn the_index_keys_a_time_of_day_by_the_lexicons_reading_of_the_form() {
        let shape = format!("element.value_string ~ '{}'", TimeValueForm::Time.lexical_pattern());
        assert!(FUNCTIONS.contains(&shape), "sub_eq_keys reads a Time by another pattern than {shape}");
        assert!(
            FUNCTIONS.contains("replace(element.value_string, ',', '.')"),
            "sub_eq_keys reads the comma as the decimal point"
        );
        assert!(
            FUNCTIONS.contains(&format!("'{STORED_TIME_TEMPLATE}') || 'Z'")),
            "sub_eq_keys keys the fixed-width spelling"
        );
    }
}
