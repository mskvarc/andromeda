//! The Evolutions stage 1 of a temporal query reads under `e`: the walk that
//! yields them in identifier order, and how a selector's type is tested on
//! what that walk yields.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{entity_selection::ConsideredEntities, temporal::compiled::CompiledTemporalQuery, type_selection::grammar::TypeExpr};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    history_source::{
        HistoryBranch,
        PendingEntries,
        entity_rows::{attribute_holder_rows, attribute_holder_rows_by_key, entity_rows, type_member_rows},
    },
    type_membership::drives,
};
use andromeda_postgresql_schema::current::E;
use sea_query::TableRef;

/// How a selector's type is tested on the Evolutions stage 1 reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeTest {
    /// A containment test on the Evolution's type keys.
    TypeKeys,
    /// Nothing: the membership walk the Evolutions come from selected the
    /// type already.
    Walked,
}

/// The Evolutions stage 1 reads under `e`.
pub(super) struct Evolutions {
    /// The Evolutions the page is cut from.
    pub(super) paged: TableRef,
    /// The same Evolutions, in the order the count probes them best.
    pub(super) counted: TableRef,
    /// How a selector's type is tested on them.
    pub(super) type_test: TypeTest,
}

impl Evolutions {
    /// Evolutions the page and the count read in the same order.
    fn read_alike(rows: TableRef, type_test: TypeTest) -> Evolutions {
        Evolutions {
            // clone: the count reads the same Evolutions
            counted: rows.clone(),
            paged: rows,
            type_test,
        }
    }
}

/// The Evolutions stage 1 reads under `e`, and how a selector's type is
/// tested on them.
///
/// The walk of the type memberships when it answers every selector's type on
/// its own: each selector names no identifier (the identifier index answers
/// one already) and a type with a range to walk, and either there is one
/// selector or none carries a pattern. The walk yields the union of the
/// selectors' types, so it cannot tell which pattern goes with which type,
/// and an Evolution only queued has no membership row to probe for it.
/// Otherwise, when the request names attributes and no selector names a type
/// or an identifier, the walk of those attributes' streams: step S1 of clause
/// 5.7.4.4 keeps only Evolutions with an instance of one of them, which hold
/// a stream of it, and no type is tested on what it yields. Its count probes
/// the same holders in the order of their surrogate keys when the queue is
/// read: each probe then reads the history of one Evolution inside the
/// union, and in key order those reads follow the indexes. A request that
/// considers no Entity (clause 5.5.14) walks nothing. Otherwise the temporal
/// catalogue, its types tested on its type keys.
pub(super) fn evolution_rows(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Evolutions {
    let selection = &compiled.entities;
    let selectors = &selection.selectors;
    match selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => return Evolutions::read_alike(entity_rows(branch, E), TypeTest::TypeKeys),
    }
    let type_walkable = !selectors.is_empty()
        && selectors.iter().all(|selector| selector.ids.is_empty())
        && (selectors.len() == 1 || selectors.iter().all(|selector| selector.id_pattern.is_none()));
    let exprs: Option<Vec<&TypeExpr>> = type_walkable
        .then(|| {
            selectors
                .iter()
                .map(|selector| selector.type_expr.as_ref().filter(|expr| drives(expr)))
                .collect()
        })
        .flatten();
    if let Some(walk) = exprs.and_then(|exprs| type_member_rows(branch, E, &exprs, &names.types)) {
        return Evolutions::read_alike(walk, TypeTest::Walked);
    }
    let holder_walkable =
        !compiled.instances.attr_filter.is_empty() && selectors.iter().all(|selector| selector.ids.is_empty() && selector.type_expr.is_none());
    if holder_walkable {
        let keys = concerned_attribute_keys(compiled, &names.attributes);
        let paged = attribute_holder_rows(branch, E, &keys);
        return match branch.pending() {
            PendingEntries::Unioned => Evolutions {
                paged,
                counted: attribute_holder_rows_by_key(branch, E, &keys),
                type_test: TypeTest::TypeKeys,
            },
            PendingEntries::Absent => Evolutions::read_alike(paged, TypeTest::TypeKeys),
        };
    }
    Evolutions::read_alike(entity_rows(branch, E), TypeTest::TypeKeys)
}

/// The dictionary keys of the attributes the request concerns (clause
/// 5.7.4.4 step S1); an attribute the dictionary never keyed has no history
/// and no key.
pub(super) fn concerned_attribute_keys(compiled: &CompiledTemporalQuery, attributes: &ResolvedNames<AttributeName>) -> Vec<AttributeNameKey> {
    compiled
        .instances
        .attr_filter
        .iter()
        .filter_map(|name| attributes.get_str(name.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{
            built,
            built_count,
            built_windowed,
            ids,
            instant,
            iri,
            minimal_compiled,
            speed_and_address,
            typed,
            vehicle,
            vehicle_is_known,
            vehicle_selector,
        },
    };
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity_selection::EntitySelector,
        page_window::PageWindow,
        pattern::Pattern,
        temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::{
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_type_name_key::EntityTypeNameKey,
        history_source::HistoryBranch,
    };
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    /// Clause 4.17: a selection by type alone walks the type's membership
    /// range, which yields its Evolutions in the clause 5.5.9 `id` order, and
    /// tests no type key array.
    #[test]
    fn a_temporal_type_selection_walks_its_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let (sql, _) = built_windowed(&compiled, &PageWindow::offset(20, 0), &vehicle_is_known());
        assert!(sql.starts_with(r#"SELECT "e"."id" AS "entity_id" FROM"#), "{sql}");
        assert!(
            sql.contains(
                r#"FROM (SELECT "temporal_entity_key", "id" FROM "temporal_entity_type_members" WHERE "entity_type_name_key" = (3) ORDER BY "id" ASC) AS "rw""#
            ),
            "{sql}"
        );
        assert!(!sql.contains(r#""e"."entity_type_name_keys""#), "{sql}");
        assert!(!sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(sql.contains(r#") AS "s" WHERE"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
    }

    /// Clause 4.17 `,`: the walk merges the ranges of both types, each
    /// Evolution once, in identifier order.
    #[test]
    fn a_type_disjunction_walks_the_merged_ranges_in_identifier_order() {
        let bus = TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Bus"));
        let mut names = vehicle_is_known();
        names.types = ResolvedNames::from_pairs([
            (
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
                EntityTypeNameKey::from(3),
            ),
            (
                "https://uri.etsi.org/ngsi-ld/default-context/Bus".parse().expect("valid type name"),
                EntityTypeNameKey::from(4),
            ),
        ]);
        let compiled = typed(vec![EntitySelector::builder().type_expr(Some(TypeExpr::Or(vec![vehicle(), bus]))).build()]);
        let (sql, _) = built(&compiled, &names);
        assert!(
            sql.contains(r#"FROM (SELECT DISTINCT ON ("typed"."id") "typed"."temporal_entity_key", "typed"."id" FROM ("#),
            "{sql}"
        );
        assert!(sql.contains(r#"ORDER BY "typed"."id" ASC, "id" ASC) AS "rw""#), "{sql}");
        assert!(
            sql.contains(r#""rq"."entity_type_name_keys" @> (ARRAY[3])) OR ("rq"."entity_type_name_keys" @> (ARRAY[4]))"#),
            "{sql}"
        );
    }

    /// A Snapshot has no queue, so its type walk is the membership range alone.
    #[test]
    fn a_snapshot_type_selection_walks_its_own_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let sql = build_stage1_entity_ids(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Omit,
            &vehicle_is_known(),
        )
        .expect("valid temporal query")
        .select
        .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"FROM (SELECT "temporal_entity_key", "id" FROM "snap_7"."temporal_entity_type_members" WHERE "entity_type_name_key" = (3)) AS "e""#),
            "{sql}"
        );
    }

    /// The count of a type selection counts the same walk the page cuts.
    #[test]
    fn a_counted_type_selection_counts_its_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let count = built_count(&compiled, &vehicle_is_known());
        assert!(count.starts_with(r#"SELECT COUNT("e"."id") FROM"#), "{count}");
        assert!(
            count.contains(r#""temporal_entity_type_members" WHERE "entity_type_name_key" = (3)"#),
            "{count}"
        );
    }

    /// A lone selector's pattern still filters the walk of its type.
    #[test]
    fn a_lone_typed_selector_keeps_its_pattern_over_the_walk() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .type_expr(Some(vehicle()))
                .build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(sql.contains(r#""temporal_entity_type_members""#), "{sql}");
        assert!(sql.contains(r#""e"."id" ~ "#), "{sql}");
        assert!(!sql.contains(r#""e"."entity_type_name_keys""#), "{sql}");
    }

    /// A selector naming identifiers is answered by the identifier index, so
    /// it keeps the catalogue and tests the type on its type keys.
    #[test]
    fn a_typed_selector_naming_identifiers_keeps_the_catalogue() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .ids(ids(&["urn:ngsi-ld:Vehicle:1"]))
                .type_expr(Some(vehicle()))
                .build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
    }

    /// The walk yields the union of the selectors' types, so it cannot pair a
    /// pattern with its own selector's type: such selectors keep the
    /// catalogue.
    #[test]
    fn several_selectors_pairing_types_with_patterns_keep_the_catalogue() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .type_expr(Some(vehicle()))
                .build(),
            vehicle_selector(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
    }

    /// A selector with no type has no range, so neither has the selection.
    #[test]
    fn a_selection_with_an_untyped_selector_keeps_the_catalogue() {
        let compiled = typed(vec![
            vehicle_selector(),
            EntitySelector::builder().id_pattern(Some(Pattern::parse(".*").expect("valid pattern"))).build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
    }

    /// A query concerning `speed` alone over the window from 2024.
    fn speed_only() -> CompiledTemporalQuery {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        compiled.instances.attr_filter = vec![iri("https://example.com/speed")];
        compiled
    }

    /// Clause 5.7.4.4 step S1 keeps only the Evolutions with an instance of a
    /// concerned attribute, which hold a stream of it, so a selection by
    /// `attrs` alone walks that attribute's streams in the clause 5.5.9 `id`
    /// order and reads no other Evolution.
    #[test]
    fn a_temporal_attrs_only_selection_reads_the_attribute_streams() {
        let (sql, _) = built(&speed_only(), &speed_and_address());
        assert!(
            sql.starts_with(concat!(
                r#"SELECT "e"."id" AS "entity_id" FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key" FROM "#,
                r#"(SELECT "hr"."id", "hr"."temporal_entity_key" FROM (SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC) AS "hr""#
            )),
            "{sql}"
        );
        assert!(!sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(sql.contains(r#") AS "ah" WHERE "ah"."attribute_name_key" IN ($"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "e"."id" ASC LIMIT $3 OFFSET $4"#), "{sql}");
    }

    /// The count totals the Evolutions the page walks; reading the queue, it
    /// probes them in the order of their surrogate keys.
    #[test]
    fn a_counted_attrs_only_selection_counts_the_attribute_walk() {
        let count = built_count(&speed_only(), &speed_and_address());
        assert!(
            count.starts_with(
                r#"SELECT COUNT("e"."id") FROM (SELECT "hm"."id" FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key""#
            ),
            "{count}"
        );
        assert!(
            count.contains(r#"(SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC)"#),
            "{count}"
        );
        assert!(count.contains(r#"ORDER BY "hm"."temporal_entity_key" ASC) AS "e""#), "{count}");
    }

    /// Over recorded rows alone the probe is compared beside its source, and
    /// the count reads the holders as the page does.
    #[test]
    fn a_counted_attrs_only_selection_over_recorded_rows_counts_the_page_walk() {
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live_recorded(),
            &speed_only(),
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &speed_and_address(),
        )
        .expect("valid temporal query");
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        assert!(
            count.starts_with(r#"SELECT COUNT("e"."id") FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key" FROM"#),
            "{count}"
        );
        assert!(!count.contains(r#""hm""#), "{count}");
    }

    /// A pattern names no identifier the index could answer, so the walk
    /// still reads the attribute's streams and the pattern filters what it
    /// yields.
    #[test]
    fn an_attrs_selection_with_an_id_pattern_walks_the_streams_and_keeps_the_pattern() {
        let mut compiled = speed_only();
        compiled.entities.selectors = vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .build(),
        ];
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(
            sql.contains(r#""temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC"#),
            "{sql}"
        );
        assert!(sql.contains(r#""e"."id" ~ "#), "{sql}");
    }

    /// A type has its own walk, and identifiers their own index, so neither
    /// selection walks the attribute's streams.
    #[test]
    fn an_attrs_selection_beside_a_type_or_identifiers_keeps_its_own_walk() {
        let mut names = speed_and_address();
        names.types = vehicle_is_known().types;
        let mut typed = speed_only();
        typed.entities.selectors = vec![vehicle_selector()];
        let (sql, _) = built(&typed, &names);
        assert!(sql.contains(r#""temporal_entity_type_members""#), "{sql}");
        assert!(!sql.contains(r#"AS "held""#), "{sql}");

        let mut identified = speed_only();
        identified.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        let (sql, _) = built(&identified, &names);
        assert!(sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(!sql.contains(r#"AS "held""#), "{sql}");
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the temporal catalogue and
    /// an unfiltered query names `entities` nowhere.
    #[test]
    fn the_statement_drives_from_the_temporal_catalogue_and_never_joins_entities() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(
            sql.contains("(SELECT \"re\".\"id\", \"re\".\"entity_type_name_keys\", FALSE AS \"queued\" FROM \"temporal_entities\" AS \"re\""),
            "{sql}"
        );
        assert!(sql.contains(") AS \"e\""), "{sql}");
        assert!(sql.contains("\"temporal_attribute_streams\""), "{sql}");
        assert!(!sql.contains("\"entities\""), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    /// Every Evolution source a count over an empty queue is compared to
    /// carries the key it is compared by: the type walk, the attribute walk
    /// and the catalogue.
    #[test]
    fn every_evolution_source_a_count_compares_by_key_carries_the_key() {
        let mut typed = typed(vec![vehicle_selector()]);
        typed.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let mut attributed = speed_only();
        attributed.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let mut catalogued = minimal_compiled();
        catalogued.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        for (compiled, names, evolutions) in [
            (
                typed,
                vehicle_is_known(),
                r#"(SELECT "temporal_entity_key", "id" FROM "temporal_entity_type_members""#,
            ),
            (
                attributed,
                speed_and_address(),
                r#"SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key" FROM"#,
            ),
            (catalogued, QueryNames::default(), r#"FROM "temporal_entities" AS "e""#),
        ] {
            let count = build_stage1_entity_ids(
                &HistoryBranch::live_recorded(),
                &compiled,
                &PageWindow::offset(20, 0),
                CountPolicy::Emit,
                &names,
            )
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .to_string(PostgresQueryBuilder);
            assert!(count.contains(evolutions), "{count}");
            assert!(count.contains(r#""ah"."temporal_entity_key" = "e"."temporal_entity_key""#), "{count}");
        }
    }
}
