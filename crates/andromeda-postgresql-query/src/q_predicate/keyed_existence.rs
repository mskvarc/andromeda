//! The index path of a bare existence term (clause 4.9 `q=attr`): the
//! Entities that carry a standing instance of the attribute.
//!
//! An Entity's search row names every attribute member it carries, as
//! dictionary keys the GIN over them yields by key. A name stays listed after
//! its last instance's clause 4.22 `expiresAt` passes, until the sweep rewrites
//! the Entity, so each carrier is probed for a standing instance on the leading
//! columns of the grain constraint. The candidates are then exactly the
//! Entities the term keeps.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        local_target::{LocalTarget, local_target},
        projection_probe::exists_on_projection,
    },
};
use andromeda_ngsi_ld_query::filter::{path::QueryPath, predicate::Predicate};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::operator::array_contains_key;
use sea_query::{Condition, Query, SelectStatement};

/// `SELECT "entity_key"` over the search rows naming the attribute whose
/// Entity holds a standing instance of it, `None` unless `pred` is the
/// existence of a recorded attribute.
///
/// A path with a trailing member or a sub-attribute asks for an element inside
/// an instance, which the search row does not record, and a hop asks about the
/// Entity the Relationship names.
pub(crate) fn existence_candidates(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    let Predicate::Exists(QueryPath::Local(path)) = pred else {
        return None;
    };
    match local_target(path) {
        LocalTarget::OwnValue => {}
        LocalTarget::TrailingMember(_) | LocalTarget::NestedMember | LocalTarget::SubAttribute(_) => return None,
    }
    let attribute = names.attributes.get(&path.attr)?;
    let carriers = EntityRowRefs::unaliased(rows.schema().cloned()).keyed_by(EntitySearch::Table);
    let mut candidates = Query::select();
    candidates
        .expr(carriers.entity_key_column())
        .from(carriers.table(EntitySearch::Table))
        .and_where(array_contains_key(carriers.search_column(EntitySearch::AttributeNameKeys), attribute.get()))
        .cond_where(exists_on_projection(attribute, Condition::all(), &carriers));
    Some(candidates.take())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_existence::existence_candidates,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("speed", 7), ("source", 9), ("isParked", 11)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates_in(q: &str, rows: &EntityRowRefs) -> Option<String> {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        existence_candidates(&pred, rows, &names()).map(|select| select.to_string(PostgresQueryBuilder))
    }

    fn candidates(q: &str) -> Option<String> {
        candidates_in(q, &EntityRowRefs::unaliased(None))
    }

    /// Clause 4.22: a listed name whose every instance has expired is not a
    /// carrier, so the probe keeps only the standing instances.
    #[test]
    fn a_bare_existence_reads_the_carriers_each_probed_for_a_standing_instance() {
        let sql = candidates("\"https://example.com/speed\"").expect("keyed");
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "entity_search"."entity_key" FROM "entity_search" "#,
                r#"WHERE ("entity_search"."attribute_name_keys" @> (ARRAY[7])) "#,
                r#"AND EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 "#,
                r#"AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) "#,
                r#"AND "ca"."entity_key" = "entity_search"."entity_key")"#,
            )
        );
    }

    #[test]
    fn the_carriers_are_read_in_the_branch_schema_of_the_statement() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snapshot_1").into_iden()));
        let sql = candidates_in("\"https://example.com/speed\"", &rows).expect("keyed");
        assert!(sql.contains(r#"FROM "snapshot_1"."entity_search""#), "{sql}");
        assert!(sql.contains(r#"FROM "snapshot_1"."current_attributes" AS "ca""#), "{sql}");
    }

    #[test]
    fn an_existence_inside_an_instance_or_through_a_hop_is_not_keyed() {
        for q in [
            "\"https://example.com/speed\".\"https://example.com/source\"",
            "\"https://example.com/speed\"[en]",
            "\"https://example.com/isParked\"{\"https://example.com/speed\"}",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn an_existence_of_an_unrecorded_attribute_is_not_keyed() {
        assert_eq!(candidates("\"https://example.com/never-written\""), None);
    }

    #[test]
    fn a_comparison_is_not_an_existence() {
        assert_eq!(candidates("\"https://example.com/speed\">1"), None);
    }
}
