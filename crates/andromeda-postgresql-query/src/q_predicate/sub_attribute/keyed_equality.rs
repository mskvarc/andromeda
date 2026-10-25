//! The index path of a sub-attribute equality: the rows whose sub-attribute
//! tree holds the operand at the term's name path, read through
//! `idx_ca_sub_eq_keys`.
//!
//! A write stores the equality keys of every scalar value of an instance's
//! tree in `sub_eq_keys`, and the operand's keys are computed by the same code
//! ([`operand_keys`]), so the overlap of the two arrays is a plain GIN probe.
//! A key selects candidates only, so every candidate is rechecked by the same
//! element condition the term compiles to.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::equality_value,
        instance_row::ProbedRow,
        local_target::{LocalTarget, local_target},
        projection_probe::{Ca, of_attribute, projection_column, standing},
        sub_attribute::element_path::ElementPath,
    },
    sub_attribute_key::{key::SubAttributeKey, operand_keys::operand_keys},
};
use andromeda_ngsi_ld_query::filter::{
    path::QueryPath,
    predicate::{Predicate, SetMembership},
    scalar::CompareValue,
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{operator::hash_key_array, pg_operator::PgBinaryOperator};
use sea_query::{Query, SelectStatement};

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
    if path.sub_attrs.is_empty() {
        return None;
    }
    match local_target(path) {
        LocalTarget::OwnValue => {}
        LocalTarget::TrailingMember { .. } | LocalTarget::TargetLanguage(_) | LocalTarget::NestedMember(_) => return None,
    }
    let element = ElementPath::resolve(path, ProbedRow::Current, &names.attributes)?;
    if element.chain.is_empty() || element.field.is_some() {
        return None;
    }
    let keys = operand_keys(element.path_hash(), scalar)
        .into_iter()
        .map(SubAttributeKey::get)
        .collect::<Vec<_>>();
    let value = equality_value(SetMembership::Within, rhs, element.holder());
    let mut select = Query::select();
    select
        .expr(projection_column(CurrentAttributes::EntityKey))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(element.attribute))
        .and_where(standing())
        .and_where(projection_column(CurrentAttributes::SubEqKeys).array_overlaps(hash_key_array(&keys)))
        .cond_where(element.condition(value));
    Some(select.take())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_term::keyed_candidates,
    };
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 2), ("accuracy", 5), ("source", 7)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates(q: &str) -> Option<String> {
        let expr = parse(q).expect("parse q");
        keyed_candidates(&expr, &EntityRowRefs::unaliased(None), &names(), LanguageMatching::TagEquality)
            .map(|keyed| keyed.select.to_string(PostgresQueryBuilder))
    }

    /// The inline key array a keyed equality on `temperature.source` reads.
    fn key_array(operand: &str) -> String {
        let sql = candidates(&format!("\"https://example.com/temperature\".\"https://example.com/source\"=={operand}")).expect("keyed");
        let (_, keys) = sql.split_once(r#""ca"."sub_eq_keys" && (ARRAY["#).expect("the key column is probed");
        keys.split_once("]::bigint[])").expect("the keys are an inline bigint array").0.to_owned()
    }

    #[test]
    fn a_keyed_equality_reads_its_candidates_through_the_key_column() {
        let sql = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"").expect("keyed");
        assert!(sql.contains(r#""ca"."sub_eq_keys" && (ARRAY["#), "{sql}");
        assert!(
            sql.contains(r#"FROM UNNEST("ca"."sub_attributes") AS "l0""#),
            "the candidates are rechecked: {sql}"
        );
    }

    /// A String may equal a String or a Relationship object, keyed apart.
    #[test]
    fn a_string_operand_reads_two_keys() {
        assert_eq!(key_array("\"sensor-43\"").split(", ").count(), 2);
    }

    /// A declared time value is keyed under its kind and a `valueType`-declared
    /// member as the String it is held as, so a time operand reads both keys,
    /// the same whichever spelling the operand is written in.
    #[test]
    fn a_date_time_operand_reads_its_kind_key_and_its_string_key_whatever_its_spelling() {
        let keys = key_array("2024-01-01T00:00:00Z");
        assert_eq!(keys.split(", ").count(), 2);
        assert_eq!(key_array("2024-01-01T00:00:00.000Z"), keys);
    }

    #[test]
    fn a_time_operand_reads_its_kind_key_and_its_string_key_whatever_its_spelling() {
        let keys = key_array("12:00:00Z");
        assert_eq!(keys.split(", ").count(), 2);
        assert_eq!(key_array("12:00:00.000Z"), keys);
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
