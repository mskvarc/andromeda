//! The Evolutions stage 1 of a temporal query reads under `e`: the walk that
//! yields them in identifier order, narrowed to a candidate step's candidates
//! when the page or the count is cut from them, and how a selector's type is
//! tested on what that walk yields.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{entity_selection::ConsideredEntities, temporal::compiled::CompiledTemporalQuery, type_selection::grammar::TypeExpr};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    history_source::{
        HistoryBranch,
        PendingEntries,
        entity_rows::{WalkedKeys, entity_rows, type_member_rows},
        holder_rows::{attribute_holder_rows, attribute_holder_rows_by_key},
    },
    one_time_gate::{Candidates, column_of},
    type_membership::drives,
};
use andromeda_postgresql_schema::current::{E, Entities};
use iri_rs::IriBuf;
use sea_query::{Asterisk, Expr, ExprTrait, IntoIden, Order, Query, TableRef};

/// How a selector's type is tested on the Evolutions stage 1 reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeTest {
    /// A containment test on the Evolution's type keys.
    TypeKeys,
    /// Nothing: the membership walk the Evolutions come from selected the
    /// type already.
    Walked,
}

/// Which Evolutions of its walk stage 1 reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Among {
    /// Every Evolution the walk yields.
    Every,
    /// Those the `candidates` CTE names: by surrogate key over recorded rows
    /// alone, by identifier where the branch reads its queue.
    Candidates,
}

/// The Evolutions stage 1 reads under `e`.
pub(super) struct Evolutions {
    /// The Evolutions the page is cut from.
    pub(super) paged: TableRef,
    /// The Evolutions the count reads: those the page is cut from, in the
    /// order the count probes them best, or a wider source the count's window
    /// match narrows to exactly them.
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
/// union, and in key order those reads follow the indexes. Over recorded rows
/// alone a bounded window's count reads the catalogue instead: its window
/// match names the concerned attributes, so it keeps exactly the holders with
/// an instance in the window, and compared beside the history by key it lets
/// PostgreSQL join the window's rows to the catalogue rather than probe
/// every holder after merging the attribute ranges in identifier order. A
/// request that considers no Entity (clause 5.5.14) walks nothing. Otherwise
/// the temporal catalogue, its types tested on its type keys.
///
/// Read `among` the candidates, the walk is narrowed to them and yields them
/// in identifier order before anything tests them: over recorded rows alone
/// a membership or holder walk reads the candidates' rows of its own ranges,
/// and the catalogue the candidates' rows; where the branch reads its queue,
/// every walk keeps the Evolutions whose identifier the candidates name.
pub(super) fn evolution_rows(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames, among: Among) -> Evolutions {
    let walked = match (among, branch.pending()) {
        (Among::Candidates, PendingEntries::Absent) => WalkedKeys::Among(Box::new(column_of(Candidates, Entities::EntityKey))),
        (Among::Every, PendingEntries::Absent | PendingEntries::Unioned) | (Among::Candidates, PendingEntries::Unioned) => WalkedKeys::Every,
    };
    let (evolutions, source) = walk(branch, compiled, names, &walked);
    match (among, branch.pending(), source) {
        (Among::Every, PendingEntries::Absent | PendingEntries::Unioned, WalkSource::Ranges | WalkSource::Catalogue)
        | (Among::Candidates, PendingEntries::Absent, WalkSource::Ranges) => evolutions,
        (Among::Candidates, PendingEntries::Absent, WalkSource::Catalogue) => {
            Evolutions::read_alike(narrowed(evolutions.paged, Entities::EntityKey), evolutions.type_test)
        }
        (Among::Candidates, PendingEntries::Unioned, WalkSource::Ranges | WalkSource::Catalogue) => Evolutions {
            paged: narrowed(evolutions.paged, Entities::Id),
            counted: narrowed(evolutions.counted, Entities::Id),
            type_test: evolutions.type_test,
        },
    }
}

/// The column the Evolutions a walk yields and the candidates name each other
/// by: the surrogate key over recorded rows alone, the identifier where the
/// branch reads its queue, the one column an Evolution only queued carries.
pub(super) const fn walked_key(branch: &HistoryBranch) -> Entities {
    match branch.pending() {
        PendingEntries::Absent => Entities::EntityKey,
        PendingEntries::Unioned => Entities::Id,
    }
}

/// What a walk of the Evolutions reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkSource {
    /// Ranges of the type memberships or of the attribute streams, which
    /// narrow their recorded rows themselves.
    Ranges,
    /// The temporal catalogue.
    Catalogue,
}

/// `walk` under `e` kept to the Evolutions whose `column` the candidates
/// name, in identifier order behind `OFFSET 0`, so PostgreSQL neither merges
/// it into the statement reading it nor tests the candidates before it orders
/// them.
fn narrowed(walk: TableRef, column: Entities) -> TableRef {
    let mut narrowed = Query::select();
    narrowed
        .column(Asterisk)
        .from(walk)
        .and_where(Expr::col((E, column)).in_subquery(column_of(Candidates, column)))
        .order_by(Entities::Id, Order::Asc)
        .offset(0);
    TableRef::SubQuery(Box::new(narrowed.take()), E.into_iden())
}

/// The Evolutions of the walk stage 1 reads, the recorded rows of a
/// membership or holder walk narrowed to `walked`, and what the walk reads.
fn walk(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames, walked: &WalkedKeys) -> (Evolutions, WalkSource) {
    let selection = &compiled.entities;
    let selectors = &selection.selectors;
    let catalogue = || (Evolutions::read_alike(entity_rows(branch, E), TypeTest::TypeKeys), WalkSource::Catalogue);
    match selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => return catalogue(),
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
    if let Some(walk) = exprs.and_then(|exprs| type_member_rows(branch, E, &exprs, &names.types, walked)) {
        return (Evolutions::read_alike(walk, TypeTest::Walked), WalkSource::Ranges);
    }
    if walks_attribute_holders(compiled) {
        let keys = concerned_attribute_keys(compiled, &names.attributes);
        let paged = attribute_holder_rows(branch, E, &keys, &names.holders, walked);
        let evolutions = match (branch.pending(), walked, compiled.window.interval.is_bounded()) {
            (PendingEntries::Unioned, _, _) => Evolutions {
                paged,
                counted: attribute_holder_rows_by_key(branch, E, &keys),
                type_test: TypeTest::TypeKeys,
            },
            (PendingEntries::Absent, WalkedKeys::Every, true) => Evolutions {
                paged,
                counted: entity_rows(branch, E),
                type_test: TypeTest::TypeKeys,
            },
            (PendingEntries::Absent, WalkedKeys::Every, false) | (PendingEntries::Absent, WalkedKeys::Among(_), _) => {
                Evolutions::read_alike(paged, TypeTest::TypeKeys)
            }
        };
        return (evolutions, WalkSource::Ranges);
    }
    catalogue()
}

/// Whether stage 1 reads the Evolutions of `compiled` by walking the holders
/// of the attributes it concerns: a selection by attribute alone (clause
/// 5.7.4.4 S1), naming no identifier and no type. Its holders are paged
/// through the types the stream census names, which the statement's names
/// carry ([`QueryNames::holders`]).
#[must_use]
pub fn walks_attribute_holders(compiled: &CompiledTemporalQuery) -> bool {
    let selection = &compiled.entities;
    match selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => return false,
    }
    !compiled.instances.attr_filter.is_empty()
        && selection
            .selectors
            .iter()
            .all(|selector| selector.ids.is_empty() && selector.type_expr.is_none())
}

/// The dictionary keys of the attributes the request concerns (clause
/// 5.7.4.4 step S1); an attribute the dictionary never keyed has no history
/// and no key.
pub fn concerned_attribute_keys(compiled: &CompiledTemporalQuery, attributes: &ResolvedNames<AttributeName>) -> Vec<AttributeNameKey> {
    attributes.keys_of(compiled.instances.attr_filter.iter().map(IriBuf::as_str))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{
            PageBranches,
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
                r#"FROM (SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member" ORDER BY "id" ASC) AS "rw""#
            ),
            "{sql}"
        );
        assert!(!sql.contains(r#""e"."evolution_type_name_keys""#), "{sql}");
        assert!(!sql.contains(r#"FROM "entities" AS "re" UNION ALL"#), "{sql}");
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
            sql.contains(r#"FROM (SELECT DISTINCT ON ("typed"."id") "typed"."entity_key", "typed"."id" FROM ("#),
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
            sql.contains(
                r#"FROM (SELECT "entity_key", "id" FROM "snap_7"."entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member") AS "e""#
            ),
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
            count.contains(r#""entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member""#),
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
        assert!(sql.contains(r#""entity_type_members""#), "{sql}");
        assert!(sql.contains(r#"("e"."id") COLLATE "default") ~ "#), "{sql}");
        assert!(!sql.contains(r#""e"."evolution_type_name_keys""#), "{sql}");
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
        assert!(!sql.contains("entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."evolution_type_name_keys" @> (ARRAY[3])"#), "{sql}");
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
        assert!(!sql.contains("entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."evolution_type_name_keys" @> (ARRAY[3])"#), "{sql}");
    }

    /// A selector with no type has no range, so neither has the selection.
    #[test]
    fn a_selection_with_an_untyped_selector_keeps_the_catalogue() {
        let compiled = typed(vec![
            vehicle_selector(),
            EntitySelector::builder().id_pattern(Some(Pattern::parse(".*").expect("valid pattern"))).build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("entity_type_members"), "{sql}");
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
    /// `attrs` alone reads the holders of that attribute's streams in the
    /// clause 5.5.9 `id` order and reads no other Evolution; `speed` has few
    /// holders, which are read whole and sorted.
    #[test]
    fn a_temporal_attrs_only_selection_reads_the_attribute_streams() {
        let (sql, _) = built(&speed_only(), &speed_and_address());
        let walk = PageBranches::of(&sql).walk;
        assert!(
            walk.starts_with(concat!(
                r#"AS "indexed") UNION ALL (SELECT * FROM (SELECT "e"."id" AS "entity_id" FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."entity_key" FROM "#,
                r#"(SELECT "hr"."id", "hr"."entity_key" FROM (SELECT "hi"."id", "hs"."entity_key" FROM "temporal_attribute_streams" AS "hs" "#,
                r#"JOIN "entities" AS "hi" ON "hi"."entity_key" = "hs"."entity_key" WHERE "hs"."attribute_name_key" = (7) ORDER BY "hi"."id" ASC) AS "hr""#
            )),
            "{walk}"
        );
        assert!(!sql.contains(r#"FROM "entities" AS "re" UNION ALL"#), "{sql}");
        assert!(walk.contains(r#") AS "ah" WHERE "ah"."attribute_name_key" = ANY($"#), "{walk}");
        assert!(walk.contains(r#"ORDER BY "e"."id" ASC LIMIT $"#), "{walk}");
    }

    /// An attribute the census counts many streams of is walked through the
    /// Evolution memberships of the types holding it, which yield their
    /// members in the clause 5.5.9 `id` order, rather than read whole.
    #[test]
    fn a_temporal_attrs_only_selection_of_a_dense_attribute_walks_the_members_of_its_holding_types() {
        let mut compiled = speed_only();
        compiled.instances.attr_filter = vec![iri("https://example.com/address")];
        let (sql, _) = built(&compiled, &speed_and_address());
        let walk = PageBranches::of(&sql).walk;
        assert!(
            walk.contains(r#"FROM "entity_type_members" AS "hw" WHERE "hw"."entity_type_name_key" = (3) AND "hw"."is_evolution_member""#),
            "{walk}"
        );
        assert!(!walk.contains(r#"JOIN "entities" AS "hi""#), "{walk}");
    }

    /// The count totals the Evolutions the page walks, probing them in the
    /// order of their surrogate keys, each holder once; a queued holder, with
    /// no key yet, comes last.
    #[test]
    fn a_counted_attrs_only_selection_counts_the_holders_in_key_order() {
        let count = built_count(&speed_only(), &speed_and_address());
        assert!(
            count.starts_with(concat!(
                r#"SELECT COUNT("e"."id") FROM (SELECT "hm"."id" FROM (SELECT DISTINCT ON ("hs"."entity_key") "hi"."id", "hs"."entity_key" "#,
                r#"FROM "temporal_attribute_streams" AS "hs" JOIN "entities" AS "hi" ON "hi"."entity_key" = "hs"."entity_key" "#,
                r#"WHERE "hs"."attribute_name_key" = ANY($1) ORDER BY "hs"."entity_key" ASC) AS "hm" UNION ALL (SELECT "hq"."id", NULL::bigint AS "entity_key" FROM "#
            )),
            "{count}"
        );
    }

    /// The count of `compiled` over `branch`, rendered with its values inlined.
    fn recorded_count(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> String {
        build_stage1_entity_ids(branch, compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, &speed_and_address())
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 5.7.4.4 step S1 keeps an Evolution with an instance of a
    /// concerned attribute in the window, which the window match names by
    /// attribute key: over recorded rows alone the count reads the catalogue
    /// under that match, compared by key beside the history, and merges no
    /// holder range.
    #[test]
    fn a_counted_attrs_only_selection_over_recorded_rows_counts_the_catalogue_under_the_window_match() {
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_7").into_iden())] {
            let count = recorded_count(&branch, &speed_only());
            assert!(count.starts_with(r#"SELECT COUNT("e"."id") FROM "#), "{count}");
            assert!(
                count.contains(r#""entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e" WHERE"#),
                "{count}"
            );
            assert!(!count.contains(r#"AS "held""#), "{count}");
            assert!(
                count.contains(concat!(
                    r#") AS "ah" WHERE "ah"."attribute_name_key" = ANY(ARRAY [7]) AND ("ah"."observed_at" >= '2024-01-01 00:00:00.000000 +00:00'"#,
                    r#" AND COALESCE("ah"."observed_at", "ah"."deleted_at") >= '2024-01-01 00:00:00.000000 +00:00')"#,
                    r#" AND "ah"."entity_key" = "e"."entity_key")"#
                )),
                "{count}"
            );
        }
    }

    /// The window match of an attribute the dictionary never keyed matches
    /// nothing, so the catalogue count answers none.
    #[test]
    fn a_counted_selection_of_an_attribute_never_keyed_matches_nothing_in_the_catalogue() {
        let mut compiled = speed_only();
        compiled.instances.attr_filter = vec![iri("https://example.com/unknown")];
        let count = recorded_count(&HistoryBranch::live_recorded(), &compiled);
        assert!(
            count.contains(r#""entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e" WHERE"#),
            "{count}"
        );
        assert!(count.contains(r#") AS "ah" WHERE (FALSE"#), "{count}");
    }

    /// An unbounded window reads no history, so every holder is a match and
    /// the count reads the holders as the page does.
    #[test]
    fn a_counted_attrs_only_selection_over_an_unbounded_window_counts_the_holders() {
        let mut compiled = speed_only();
        compiled.window.interval = TimeInterval::Unbounded;
        let count = recorded_count(&HistoryBranch::live_recorded(), &compiled);
        assert!(
            count.starts_with(r#"SELECT COUNT("e"."id") FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."entity_key" FROM"#),
            "{count}"
        );
        assert!(!count.contains(r#""entities" AS "e""#), "{count}");
    }

    /// A pattern names no identifier the index could answer, so the walk
    /// still reads the attribute's holders and the pattern filters what it
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
            sql.contains(r#"WHERE "hs"."attribute_name_key" = (7) ORDER BY "hi"."id" ASC) AS "hr""#),
            "{sql}"
        );
        assert!(sql.contains(r#"("e"."id") COLLATE "default") ~ "#), "{sql}");
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
        assert!(sql.contains(r#""entity_type_members""#), "{sql}");
        assert!(!sql.contains(r#"AS "held""#), "{sql}");

        let mut identified = speed_only();
        identified.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        let (sql, _) = built(&identified, &names);
        assert!(
            sql.contains(r#"FROM "entities" AS "re" WHERE "re"."evolution_created_at" IS NOT NULL UNION ALL"#),
            "{sql}"
        );
        assert!(!sql.contains(r#"AS "held""#), "{sql}");
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the identity rows holding
    /// an Evolution, and an unfiltered query reads no current-state relation.
    #[test]
    fn the_statement_drives_from_the_evolutions_and_never_reads_current_state() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(
            sql.contains(concat!(
                r#"(SELECT "re"."id", "re"."evolution_type_name_keys", FALSE AS "queued" FROM "entities" AS "re" "#,
                r#"WHERE "re"."evolution_created_at" IS NOT NULL"#
            )),
            "{sql}"
        );
        assert!(sql.contains(") AS \"e\""), "{sql}");
        assert!(sql.contains("\"temporal_attribute_streams\""), "{sql}");
        for current_state in ["\"entity_search\"", "\"current_attributes\"", "\"created_at\" IS NOT NULL"] {
            assert!(!sql.contains(current_state), "{current_state}: {sql}");
        }
    }

    /// Every Evolution source a count over an empty queue is compared to
    /// carries the key it is compared by: the type walk, and the catalogue an
    /// attribute selection or no selection reads.
    #[test]
    fn every_evolution_source_a_count_compares_by_key_carries_the_key() {
        let mut typed = typed(vec![vehicle_selector()]);
        typed.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let mut attributed = speed_only();
        attributed.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let mut catalogued = minimal_compiled();
        catalogued.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        for (compiled, names, evolutions) in [
            (typed, vehicle_is_known(), r#"(SELECT "entity_key", "id" FROM "entity_type_members""#),
            (
                attributed,
                speed_and_address(),
                r#"FROM (SELECT * FROM "entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e""#,
            ),
            (
                catalogued,
                QueryNames::default(),
                r#"FROM (SELECT * FROM "entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e""#,
            ),
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
            assert!(count.contains(r#""ah"."entity_key" = "e"."entity_key""#), "{count}");
        }
    }
}
