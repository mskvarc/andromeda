//! The index path of a Linked Entity hop (clause 4.9 `rel{attr}`): the
//! Entities whose Relationship names a linked entity the inner term keeps.
//!
//! As a filter, a hop probes each Entity's Relationship instances outward to
//! the entities they name, so its cost follows the selection. The inner term
//! is a test its own index answers, and an index over the object reaches the
//! instances naming each entity it keeps, so read inward the candidates cost
//! the linked matches and the instances naming them. Each candidate is a match
//! as it is read.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{keyed_branches::union_of, projection::linked_condition, traversal::LinkedHop},
};
use andromeda_ngsi_ld_query::filter::{
    path::QueryPath,
    predicate::{Predicate, SetMembership},
};
use sea_query::SelectStatement;

/// `SELECT "entity_key"` over the Entities whose standing Relationship
/// instance names a linked entity `pred` keeps, `None` unless `pred` is a
/// single hop of a recorded Relationship whose inner term an index can
/// enumerate.
pub(crate) fn hop_candidates(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    let QueryPath::Linked {
        relationship,
        guard_types,
        inner,
    } = pred.path()
    else {
        return None;
    };
    if !enumerates_linked_matches(pred, inner) {
        return None;
    }
    let relationship_id = names.attributes.get(relationship)?;
    let hop = LinkedHop::at_depth(1);
    let inner_rows = hop.inner_rows(rows.schema().cloned());
    let inner_condition = linked_condition(pred, inner, &inner_rows, names, 1);
    union_of(hop.linking_branches(relationship_id, guard_types, &names.types, rows, inner_condition))
}

/// Whether the linked entities `pred` keeps along `inner` can be read before
/// the Entities linking to them.
///
/// A negated term (`!=`, `!~=`) holds for an entity none of whose values
/// match, which no index of values enumerates, and a further hop is itself a
/// probe outward from every entity, so either would read every entity of the
/// tenant before the first candidate.
const fn enumerates_linked_matches(pred: &Predicate, inner: &QueryPath) -> bool {
    let positive = match pred {
        Predicate::Exists(_)
        | Predicate::Equality {
            membership: SetMembership::Within,
            ..
        }
        | Predicate::Ordering { .. }
        | Predicate::MatchPattern { .. } => true,
        Predicate::Equality {
            membership: SetMembership::Outside,
            ..
        }
        | Predicate::NotMatchPattern { .. } => false,
    };
    positive && !inner.is_linked()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        entity_type_name_key::EntityTypeNameKey,
        q_predicate::keyed_hop::hop_candidates,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("hasTrip", 34), ("headSign", 28), ("hasRoute", 26)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs(
                [("Trip", 10), ("Bus", 12)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), EntityTypeNameKey::from(key))),
            ),
            ..QueryNames::default()
        }
    }

    fn candidates_in(q: &str, rows: &EntityRowRefs) -> Option<String> {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        hop_candidates(&pred, rows, &names()).map(|select| select.to_string(PostgresQueryBuilder))
    }

    fn candidates(q: &str) -> Option<String> {
        candidates_in(q, &EntityRowRefs::unaliased(None))
    }

    const HOP: &str = r#""https://example.com/hasTrip"{"https://example.com/headSign"}=="Vajnory""#;

    #[test]
    fn a_hop_reads_the_instances_naming_the_linked_entities_its_inner_term_keeps() {
        let sql = candidates(HOP).expect("keyed");
        assert!(sql.starts_with(r#"SELECT "entity_key" FROM (SELECT "ca_ln1"."entity_key" FROM "entities" AS "le1" JOIN "current_attributes" AS "ca_ln1" ON "ca_ln1"."attribute_name_key" = 34 AND "ca_ln1"."value_uri" = "le1"."id" "#), "{sql}");
        assert!(sql.contains(r#"UNION ALL (SELECT "ca_ln1"."entity_key" FROM "entities" AS "le1" JOIN "current_attributes" AS "ca_ln1" ON "ca_ln1"."attribute_name_key" = 34 AND ("ca_ln1"."value_object" @> jsonb_build_array("le1"."id")) "#), "{sql}");
        assert!(sql.ends_with(r#") AS "matches""#), "{sql}");
        assert!(!sql.contains("value_long_uri"), "{sql}");
    }

    /// Clause 4.22: a past-due Relationship instance links nothing, and only
    /// a `Relationship` or a `ListRelationship` holds objects.
    #[test]
    fn each_branch_keeps_the_standing_relationship_instances_and_the_inner_term() {
        let sql = candidates(HOP).expect("keyed");
        for fragment in [
            r#""ca_ln1"."attribute_kind" IN (2, 8)"#,
            r#"("ca_ln1"."expires_at" IS NULL OR "ca_ln1"."expires_at" > NOW())"#,
            r#"FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 28"#,
            r#""ca"."entity_key" = "le1"."entity_key""#,
        ] {
            assert_eq!(sql.matches(fragment).count(), 2, "{fragment}: {sql}");
        }
    }

    #[test]
    fn a_type_guard_reads_the_linked_entitys_search_row_in_each_branch() {
        let sql =
            candidates(r#""https://example.com/hasTrip"{"https://example.com/Trip","https://example.com/Bus":"https://example.com/headSign"}=="Vajnory""#)
                .expect("keyed");
        assert_eq!(
            sql.matches(r#"JOIN "entity_search" AS "ls1" ON "ls1"."entity_key" = "le1"."entity_key""#)
                .count(),
            2,
            "{sql}"
        );
        assert_eq!(sql.matches(r#""ls1"."entity_type_name_keys" @> (ARRAY[10])"#).count(), 2, "{sql}");
    }

    #[test]
    fn the_linked_entities_are_read_in_the_branch_schema_of_the_statement() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snapshot_1").into_iden()));
        let sql = candidates_in(HOP, &rows).expect("keyed");
        assert!(
            sql.contains(r#"FROM "snapshot_1"."entities" AS "le1" JOIN "snapshot_1"."current_attributes" AS "ca_ln1""#),
            "{sql}"
        );
        assert!(sql.contains(r#"FROM "snapshot_1"."current_attributes" AS "ca""#), "{sql}");
    }

    #[test]
    fn a_positive_inner_term_of_any_operator_is_keyed() {
        for q in [
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}"#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}>3"#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}~="Vaj.*""#,
        ] {
            assert!(candidates(q).is_some(), "{q}");
        }
    }

    #[test]
    fn a_negated_inner_term_or_a_further_hop_is_not_keyed() {
        for q in [
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}!="Vajnory""#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}!~="Vaj.*""#,
            r#""https://example.com/hasTrip"{"https://example.com/hasRoute"{"https://example.com/headSign"}}=="Vajnory""#,
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn a_hop_through_an_unrecorded_relationship_or_a_local_term_is_not_keyed() {
        for q in [
            r#""https://example.com/never-written"{"https://example.com/headSign"}=="Vajnory""#,
            r#""https://example.com/headSign"=="Vajnory""#,
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }
}
