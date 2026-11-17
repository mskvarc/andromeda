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
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::equals_any_value,
        instance_row::ProbedRow,
        projection_probe::{Ca, of_attribute, projection_column, standing},
        resolved_filter::ResolvedTerm,
        sub_attribute::element_path::ElementPath,
    },
    sub_attribute_operand_keys::operand_keys,
};
use andromeda_ngsi_ld_query::filter::reading::{operand::Operand, terminal::Terminal};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{operator::hash_key_array, pg_operator::PgBinaryOperator};
use andromeda_postgresql_stored_values::sub_attribute_key::key::SubAttributeKey;
use sea_query::{Query, SelectStatement};
use std::slice;

/// `SELECT "ca"."entity_key"` over the rows whose sub-attribute tree holds
/// `operand` at the name path of `term`, the candidates of `term` asserting
/// `==` with that one operand; `None` unless the key index answers it: the
/// term tests a sub-attribute's own value, past no hop.
pub(crate) fn equality_candidates(term: &ResolvedTerm<'_>, operand: &Operand, rows: &EntityRowRefs) -> Option<SelectStatement> {
    let reading = term.term();
    if !reading.hops().is_empty() || reading.sub_attributes().is_empty() || !matches!(reading.terminal(), Terminal::OwnValue) {
        return None;
    }
    let element = ElementPath::of_resolved(term, ProbedRow::Current(rows.standing()));
    let keys = operand_keys(element.path_hash(), operand)
        .into_iter()
        .map(SubAttributeKey::get)
        .collect::<Vec<_>>();
    let value = equals_any_value(slice::from_ref(operand), element.holder());
    let mut select = Query::select();
    select
        .expr(projection_column(CurrentAttributes::EntityKey))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(element.attribute))
        .and_where(standing(rows))
        .and_where(projection_column(CurrentAttributes::SubEqKeys).array_overlaps(hash_key_array(&keys)))
        .cond_where(element.condition(value));
    Some(select.take())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        one_time_gate::CandidateBound,
        q_predicate::{keyed_term::keyed_candidates, lowered_filter::reading_of},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, SelectStatement};

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

    fn candidates(q: &str) -> Option<SelectStatement> {
        keyed_candidates(&reading_of(q), &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole)
            .into_iter()
            .next()
            .map(|keyed| keyed.select)
    }

    /// The statement of a keyed equality on `temperature.source` with `operand`.
    fn keyed_source(operand: &str) -> SelectStatement {
        candidates(&format!("\"https://example.com/temperature\".\"https://example.com/source\"=={operand}")).expect("keyed")
    }

    /// The inline key array a keyed equality on `temperature.source` reads.
    fn key_array(operand: &str) -> String {
        let sql = keyed_source(operand).to_string(PostgresQueryBuilder);
        let (_, keys) = sql.split_once(r#""ca"."sub_eq_keys" && (ARRAY["#).expect("the key column is probed");
        keys.split_once("]::bigint[])").expect("the keys are an inline bigint array").0.to_owned()
    }

    #[test]
    fn a_keyed_equality_reads_its_candidates_through_the_key_column() {
        let statement = candidates("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"").expect("keyed");
        assert_statement_snapshot!("keyed-equality-a-keyed-equality-reads-its-candidates-through-the-key-column", statement);
    }

    /// A String may equal a String or a Relationship object, keyed apart.
    #[test]
    fn a_string_operand_reads_two_keys() {
        assert_statement_snapshot!("keyed-equality-a-string-operand-reads-two-keys", keyed_source("\"sensor-43\""));
    }

    /// A declared time value is keyed under its kind and a `valueType`-declared
    /// member as the String it is held as, so a time operand reads both keys,
    /// the same whichever spelling the operand is written in.
    #[test]
    fn a_date_time_operand_reads_its_kind_key_and_its_string_key_whatever_its_spelling() {
        assert_statement_snapshot!(
            "keyed-equality-a-date-time-operand-reads-its-kind-key-and-its-string-key-whatever-its-spelling-1",
            keyed_source("2024-01-01T00:00:00Z")
        );
        assert_statement_snapshot!(
            "keyed-equality-a-date-time-operand-reads-its-kind-key-and-its-string-key-whatever-its-spelling-2",
            keyed_source("2024-01-01T00:00:00.000Z")
        );
        assert_eq!(key_array("2024-01-01T00:00:00.000Z"), key_array("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn a_time_operand_reads_its_kind_key_and_its_string_key_whatever_its_spelling() {
        assert_statement_snapshot!(
            "keyed-equality-a-time-operand-reads-its-kind-key-and-its-string-key-whatever-its-spelling-1",
            keyed_source("12:00:00Z")
        );
        assert_statement_snapshot!(
            "keyed-equality-a-time-operand-reads-its-kind-key-and-its-string-key-whatever-its-spelling-2",
            keyed_source("12:00:00.000Z")
        );
        assert_eq!(key_array("12:00:00.000Z"), key_array("12:00:00Z"));
    }

    #[test]
    fn a_sub_attribute_ordering_complement_value_list_or_disjunct_is_not_keyed() {
        for q in [
            "\"https://example.com/temperature\".\"https://example.com/accuracy\">0.5",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"!=0.5",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5,0.6",
            "\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5|\"https://example.com/temperature\"==1",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
        assert!(candidates("\"https://example.com/temperature\"==1;\"https://example.com/temperature\".\"https://example.com/accuracy\"==0.5").is_some());
    }
}
