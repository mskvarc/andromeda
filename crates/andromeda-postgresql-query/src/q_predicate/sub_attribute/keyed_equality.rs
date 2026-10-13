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
//!
//! The path is bounded: it takes at most [`CANDIDATE_BOUND`] + 1 candidates.
//! The planner underestimates a broad sub-attribute predicate by orders of
//! magnitude, and a page whose matches are many is found sooner by walking the
//! selection in page order than by fetching every match through the index, so
//! the statement that cuts the page takes the candidates only when they number
//! at most [`CANDIDATE_BOUND`].

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
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::{
    expression::Expr,
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
use std::slice::from_ref;

/// The most candidates the index path hands to the page.
pub const CANDIDATE_BOUND: u64 = 1000;

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

/// One sub-attribute equality of a `q` expression's top-level conjunction
/// and the candidates the index path reads for it.
#[derive(Debug)]
pub struct KeyedCandidates<'q> {
    /// The term the candidates answer, which the page leaves out of the
    /// filter it applies to them.
    pub term: &'q Predicate,
    /// `SELECT "ca"."entity_key" …`, at most [`CANDIDATE_BOUND`] + 1 rows.
    pub select: SelectStatement,
}

/// The first term of `expr`'s top-level conjunction the index path answers,
/// with its candidates read in the branch `rows` names.
///
/// Only a conjunct can narrow the page to its candidates: an entity a
/// disjunction keeps for another arm need not be one of them. A term
/// qualifies when it is `==` with one operand on a sub-attribute's own value,
/// every name of its path recorded.
#[must_use]
pub fn keyed_candidates<'q>(expr: &'q Expr, rows: &EntityRowRefs, names: &QueryNames) -> Option<KeyedCandidates<'q>> {
    let conjuncts = match expr {
        Expr::And(terms) => terms.as_slice(),
        Expr::Or(_) | Expr::Pred(_) => from_ref(expr),
    };
    conjuncts.iter().find_map(|term| match term {
        Expr::Pred(pred) => candidates_of(pred, rows, names).map(|select| KeyedCandidates { term: pred, select }),
        Expr::And(_) | Expr::Or(_) => None,
    })
}

/// The candidate statement of one term, `None` when the index cannot answer
/// it.
fn candidates_of(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
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
        .cond_where(element.condition(value))
        .limit(CANDIDATE_BOUND + 1);
    Some(select.take())
}

/// The keys `scalar` has at `path`: every key a stored value equal to it can
/// have.
///
/// A String or a URI operand may equal a String or a Relationship object, and
/// the two are salted apart, so both keys are read. A `DateTime` is held as an
/// instant in a scalar element and as its text inside an array, so both are
/// read too. A `Date` is held as its one text. A `Time` compares in a scalar
/// element by its canonical fixed-width text, which the element is keyed by,
/// and inside an array by the operand's text, so both are read.
fn query_keys(path: &[i32], scalar: &Scalar) -> SimpleExpr {
    let path = || key_array(path);
    let key = |value: SimpleExpr| sub_eq_query_key(path(), value);
    let text_key = |kind: i16, text: String| sub_eq_query_text_key(path(), kind, text);
    match scalar {
        // clone: sea-query owns every value in its AST
        Scalar::Number(number) => key(cast(SeaExpr::val(number.decimal().clone()), PostgresType::Numeric)),
        Scalar::Bool(value) => key(cast(SeaExpr::val(*value), PostgresType::Boolean)),
        Scalar::Str(value) | Scalar::Uri(value) => array_cat(text_key(STRING_KIND, value.to_owned()), text_key(URI_KIND, value.to_owned())),
        Scalar::DateTime(value) => array_cat(
            key(cast(SeaExpr::val(*value), PostgresType::Timestamptz)),
            text_key(STRING_KIND, scalar.literal_text()),
        ),
        Scalar::Date(value) => text_key(STRING_KIND, NgsiLdDate::from(*value).to_string()),
        Scalar::Time(value) => {
            let literal = text_key(STRING_KIND, scalar.literal_text());
            match NgsiLdTime::normalise(*value) {
                Ok(canonical) => array_cat(text_key(STRING_KIND, canonical.to_string()), literal),
                Err(_) => literal,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::sub_attribute::keyed_equality::{key_expression, keyed_candidates},
    };
    use andromeda_ngsi_ld_query::filter::parser::parse;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    /// The `current_attributes` migration, which declares `idx_ca_sub_eq_keys`.
    const MIGRATION: &str = include_str!("../../../../../migrations/branch/0024_current_attributes.sql");

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
    fn a_keyed_equality_reads_its_candidates_through_the_index_bounded() {
        let sql = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"").expect("keyed");
        assert!(
            sql.contains(r#"sub_eq_keys("ca"."attribute_name_key", "ca"."sub_attributes") && array_cat(sub_eq_query_key(ARRAY[2, 7]::integer[], 2::smallint, 'sensor-43'::text), sub_eq_query_key(ARRAY[2, 7]::integer[], 7::smallint, 'sensor-43'::text))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM UNNEST("ca"."sub_attributes") AS "l0""#),
            "the candidates are rechecked: {sql}"
        );
        assert!(sql.ends_with("LIMIT 1001"), "{sql}");
    }

    #[test]
    fn a_time_equality_reads_the_canonical_key_and_the_written_key() {
        let sql = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==12:00:00Z").expect("keyed");
        assert!(
            sql.contains(r"array_cat(sub_eq_query_key(ARRAY[2, 7]::integer[], 2::smallint, '12:00:00.000000Z'::text), sub_eq_query_key(ARRAY[2, 7]::integer[], 2::smallint, '12:00:00Z'::text))"),
            "{sql}"
        );
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
}
