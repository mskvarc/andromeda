//! Whether a Context Source Registration covers a concrete entity (clause 5.10.2).
//!
//! The forwarding decision asks the inverse of discovery: not "which
//! registrations did a client ask for" but "which registrations declared they
//! hold a given entity". A registration's `information[].entities[]` may
//! constrain the identifier by `id` or `idPattern` and the kind by `type`, and
//! an entry that constrains an axis not at all covers every value of it — so a
//! registration with no `entities`, or an empty list, covers everything.
//!
//! The `type` predicate is shared with the clause-5.9 discovery query, which
//! asks the same "does any entry declare this type" question, so the two cannot
//! drift on what a type registration means.

use sea_query::{Expr as SeaExpr, SimpleExpr};

/// Whether a registration's `information[].entities[]` covers `entity_type`.
///
/// A registration whose `information` entry declares no `entities`, or an empty
/// list, places no type constraint and covers every type; otherwise at least
/// one entry must declare the type.
pub fn entity_type_coverage(entity_type: &str) -> SimpleExpr {
    SeaExpr::cust_with_values(
        "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
         WHERE NOT jsonb_exists(info, 'entities') \
            OR info->'entities' = '[]'::jsonb \
            OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'type' = $1))",
        [entity_type.to_string()],
    )
}

/// Whether a registration's `information[].entities[]` covers `entity_id`.
///
/// Clause 5.10.2: an `EntityInfo` entry covers a concrete id when it constrains
/// the identifier axis by neither `id` nor `idPattern` (a type-only entry covers
/// any id), when its `id` equals the entity's, or when its `idPattern` regular
/// expression matches it. An `idPattern`-only entry therefore covers only the
/// ids its pattern admits, not every id — the trap a bare `NOT jsonb_exists(ent,
/// 'id')` disjunct falls into, since an entry that carries an `idPattern` but no
/// `id` would otherwise match every entity.
pub fn entity_id_coverage(entity_id: &str) -> SimpleExpr {
    SeaExpr::cust_with_values(
        "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
         WHERE NOT jsonb_exists(info, 'entities') \
            OR info->'entities' = '[]'::jsonb \
            OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE \
                   (NOT jsonb_exists(ent, 'id') AND NOT jsonb_exists(ent, 'idPattern')) \
                OR ent->>'id' = $1 \
                OR (jsonb_exists(ent, 'idPattern') AND $1 ~ (ent->>'idPattern'))))",
        [entity_id.to_string()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(predicate: SimpleExpr) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(sea_query::Alias::new("context_source_registrations"))
            .and_where(predicate)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn type_coverage_matches_a_type_only_registration_and_a_declared_type() {
        // A registration with no `entities` (the first two disjuncts) covers any
        // type, and one that declares entities covers a type it names.
        let sql = render(entity_type_coverage("https://example.com/Vehicle"));
        assert!(sql.contains("NOT jsonb_exists(info, 'entities')"), "{sql}");
        assert!(sql.contains("ent->>'type' ="), "{sql}");
    }

    #[test]
    fn id_coverage_admits_exact_id_type_only_and_idpattern_but_not_a_bare_pattern_wildcard() {
        // Clause 5.10.2: a type-only entry covers any id, an `id` covers itself,
        // an `idPattern` covers only the ids it matches. The regex disjunct is
        // what stops an idPattern-only entry from covering every id.
        let sql = render(entity_id_coverage("urn:ngsi-ld:Vehicle:1"));
        assert!(sql.contains("NOT jsonb_exists(ent, 'id') AND NOT jsonb_exists(ent, 'idPattern')"), "{sql}");
        assert!(sql.contains("ent->>'id' ="), "{sql}");
        assert!(sql.contains("jsonb_exists(ent, 'idPattern') AND"), "{sql}");
        assert!(sql.contains("~ (ent->>'idPattern')"), "the idPattern regex disjunct must be present: {sql}");
    }
}
