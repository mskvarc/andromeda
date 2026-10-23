//! The Temporal Evolutions holding streams of the attributes a temporal
//! selection by attribute alone names (clause 5.7.4.4 S1), in the clause
//! 5.5.9 identifier order or, for a count, in the order of their surrogate
//! keys.
//!
//! No stream carries the Entity's identifier. An attribute with few holders
//! is read whole from the streams' `(attribute_name_key, entity_key)` index and
//! sorted by identifier; any other is walked through the Evolution memberships
//! of the types the stream census names as holding it, which yield their
//! members in identifier order. The ranges merge with each Evolution once.

use crate::{
    attribute_name_key::AttributeNameKey,
    history_source::{HistoryBranch, HistoryOf, PendingEntries, attribute_holders::AttributeHolders, entity_rows::WalkedKeys, grain::HistoryGrain, pending},
};
use andromeda_postgresql_schema::{
    current::{Entities, EntityTypeMembers},
    temporal::TemporalAttributeStreams,
    type_membership::MembershipSide,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::typed_null,
    literal::{false_literal, int_literal},
    pg_array::is_any_of,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, JoinType, Order, Query, SelectStatement, TableRef, UnionType};

/// Alias `held`: one stream row of the merged attribute walk.
#[derive(Iden, Clone, Copy)]
#[iden = "held"]
struct HeldStream;

/// Alias `hr`: one recorded stream of an attribute's range.
#[derive(Iden, Clone, Copy)]
#[iden = "hr"]
struct RecordedHolder;

/// Alias `hs`: a stream of a recorded holder.
#[derive(Iden, Clone, Copy)]
#[iden = "hs"]
struct HolderStream;

/// Alias `hi`: the identity row of a recorded holder.
#[derive(Iden, Clone, Copy)]
#[iden = "hi"]
struct HolderIdentity;

/// Alias `hw`: a member of a holding type the holder walk visits.
#[derive(Iden, Clone, Copy)]
#[iden = "hw"]
struct HolderMember;

/// How many streams of an attribute the census may count for its holders to
/// be read whole and sorted by identifier rather than walked through the
/// memberships of their types: sorting this many rows costs a page less than
/// walking a type whose members mostly hold no stream of the attribute.
const HOLDER_SORT_BOUND: u64 = 10_000;

/// Alias `hq`: an Entity a queued grain of the attributes extends.
#[derive(Iden, Clone, Copy)]
#[iden = "hq"]
struct QueuedHolder;

/// Alias `hm`: one holder of the merged attribute walk, read in key order.
#[derive(Iden, Clone, Copy)]
#[iden = "hm"]
struct MergedHolder;

/// Alias `hg`: a grain queued entries extend that no recorded stream holds.
#[derive(Iden, Clone, Copy)]
#[iden = "hg"]
struct QueuedGrain;

/// The Temporal Evolutions of `branch` holding a stream of any attribute
/// `keys` names, under `alias`, with the columns `id` and `entity_key` (null
/// for an Evolution only queued), each once, in identifier order; no row when
/// `keys` is empty.
///
/// No stream carries the Entity's identifier, so the recorded holders of an
/// attribute come in identifier order from one of two plans, each reaching
/// the merge as a subquery stating its own order ([`holder_ranges`]). On the
/// live branch the Entities whose queued entries extend a grain of one of
/// the attributes that no recorded stream holds join the merge. Narrowed by
/// `walked`, each range reads the named Evolutions alone.
#[must_use]
pub fn attribute_holder_rows(
    branch: &HistoryBranch,
    alias: impl IntoIden,
    keys: &[AttributeNameKey],
    holders: &AttributeHolders,
    walked: &WalkedKeys,
) -> TableRef {
    let walk = holder_merge(branch, keys, holders, walked).unwrap_or_else(no_holder);
    TableRef::SubQuery(Box::new(walk), alias.into_iden())
}

/// The Temporal Evolutions holding a stream of any attribute `keys` names on
/// `branch`, under `alias`, with the column `id`, yielded in the order of
/// their surrogate key; for a count, which needs no order of its own.
///
/// Each holder's history is probed by the stream and history indexes, whose
/// entries follow the surrogate keys as the Evolutions were minted. Probed in
/// identifier order, which follows no key, a dense attribute's probes read
/// those indexes at random; in key order they read them in sequence. The
/// holders are read from `(attribute_name_key, entity_key)`, each once; a
/// queued holder, with no key yet, comes last.
#[must_use]
pub fn attribute_holder_rows_by_key(branch: &HistoryBranch, alias: impl IntoIden, keys: &[AttributeNameKey]) -> TableRef {
    if keys.is_empty() {
        return TableRef::SubQuery(Box::new(no_holder()), alias.into_iden());
    }
    let schema = branch.schema();
    let mut recorded = Query::select();
    recorded
        .distinct_on([(HolderStream, TemporalAttributeStreams::EntityKey)])
        .column((HolderIdentity, Entities::Id))
        .column((HolderStream, TemporalAttributeStreams::EntityKey))
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(HolderStream))
        .join_as(
            JoinType::Join,
            qualified(schema, Entities::Table),
            HolderIdentity,
            Expr::col((HolderIdentity, Entities::EntityKey)).equals((HolderStream, TemporalAttributeStreams::EntityKey)),
        )
        .and_where(is_any_of(
            Expr::col((HolderStream, TemporalAttributeStreams::AttributeNameKey)),
            keys.iter().map(|key| key.get()),
        ))
        .order_by((HolderStream, TemporalAttributeStreams::EntityKey), Order::Asc);
    let mut by_key = Query::select();
    by_key.column((MergedHolder, Entities::Id)).from_subquery(recorded.take(), MergedHolder);
    if let Some(queued) = queued_holders(branch, keys) {
        by_key.union(UnionType::All, queued);
    }
    TableRef::SubQuery(Box::new(by_key.take()), alias.into_iden())
}

/// The holders of `keys`' streams on `branch` among the Evolutions `walked`
/// names, each once, in identifier order, with the identifier and the
/// Entity's surrogate key, null for a holder only queued; `None` when `keys`
/// is empty.
fn holder_merge(branch: &HistoryBranch, keys: &[AttributeNameKey], holders: &AttributeHolders, walked: &WalkedKeys) -> Option<SelectStatement> {
    if keys.is_empty() {
        return None;
    }
    let mut ranges = keys
        .iter()
        .flat_map(|key| holder_ranges(branch, *key, holders, walked))
        .map(|range| {
            let mut ordered = Query::select();
            ordered
                .column((RecordedHolder, Entities::Id))
                .column((RecordedHolder, Entities::EntityKey))
                .from_subquery(range, RecordedHolder);
            ordered
        })
        .chain(queued_holders(branch, keys));
    let Some(mut union) = ranges.next() else {
        return Some(no_holder());
    };
    for range in ranges {
        union.union(UnionType::All, range);
    }
    let mut walk = Query::select();
    walk.distinct_on([(HeldStream, Entities::Id)])
        .column((HeldStream, Entities::Id))
        .column((HeldStream, Entities::EntityKey))
        .from_subquery(union.take(), HeldStream)
        .order_by((HeldStream, Entities::Id), Order::Asc);
    Some(walk.take())
}

/// The recorded holders of the attribute `key` on `branch` among the
/// Evolutions `walked` names, as `(id, entity_key)` ranges each ordered by
/// identifier; none when the census names no holding type.
///
/// An attribute whose census counts few streams ([`HOLDER_SORT_BOUND`]) is
/// read whole from `(attribute_name_key, entity_key)` and sorted by
/// identifier: a page then costs those few rows. Any other is walked through
/// the Evolution memberships of each type holding it, in identifier order,
/// keeping a member whose grain holds a stream of the attribute; the probe
/// is a scalar subquery limited to one row, which PostgreSQL keeps a probe
/// per walked member, where an `EXISTS` is turned into a semi join reading
/// every holder of the attribute and sorting them.
fn holder_ranges(branch: &HistoryBranch, key: AttributeNameKey, holders: &AttributeHolders, walked: &WalkedKeys) -> Vec<SelectStatement> {
    let schema = branch.schema();
    let types = holders.of(key);
    if types.is_empty() {
        return Vec::new();
    }
    if holders.streams_of(key) <= HOLDER_SORT_BOUND {
        let mut sorted = Query::select();
        sorted
            .column((HolderIdentity, Entities::Id))
            .column((HolderStream, TemporalAttributeStreams::EntityKey))
            .from(qualified(schema, TemporalAttributeStreams::Table).alias(HolderStream))
            .join_as(
                JoinType::Join,
                qualified(schema, Entities::Table),
                HolderIdentity,
                Expr::col((HolderIdentity, Entities::EntityKey)).equals((HolderStream, TemporalAttributeStreams::EntityKey)),
            )
            .and_where(Expr::col((HolderStream, TemporalAttributeStreams::AttributeNameKey)).eq(int_literal(i64::from(key.get()))));
        walked.narrow(&mut sorted, Expr::col((HolderStream, TemporalAttributeStreams::EntityKey)));
        sorted.order_by((HolderIdentity, Entities::Id), Order::Asc);
        return vec![sorted.take()];
    }
    types
        .iter()
        .map(|holder| {
            let mut stream = Query::select();
            stream
                .expr(int_literal(1))
                .from(qualified(schema, TemporalAttributeStreams::Table).alias(HolderStream))
                .and_where(Expr::col((HolderStream, TemporalAttributeStreams::EntityKey)).equals((HolderMember, EntityTypeMembers::EntityKey)))
                .and_where(Expr::col((HolderStream, TemporalAttributeStreams::AttributeNameKey)).eq(int_literal(i64::from(key.get()))))
                .limit(1);
            let mut members = Query::select();
            members
                .column((HolderMember, EntityTypeMembers::Id))
                .column((HolderMember, EntityTypeMembers::EntityKey))
                .from(qualified(schema, EntityTypeMembers::Table).alias(HolderMember))
                .and_where(Expr::col((HolderMember, EntityTypeMembers::EntityTypeNameKey)).eq(int_literal(i64::from(holder.type_key.get()))))
                .and_where(Expr::col((HolderMember, MembershipSide::Evolution.flag())))
                .and_where(scalar_subquery(stream.take()).is_not_null());
            walked.narrow(&mut members, Expr::col((HolderMember, EntityTypeMembers::EntityKey)));
            members.order_by((HolderMember, EntityTypeMembers::Id), Order::Asc);
            members.take()
        })
        .collect()
}

/// The Entities whose queued entries extend a grain of one of `keys` that
/// no stream recorded on `branch` holds, as `(id, entity_key)` with a null
/// key, ordered by identifier; `None` where the branch reads no queue.
fn queued_holders(branch: &HistoryBranch, keys: &[AttributeNameKey]) -> Option<SelectStatement> {
    match branch.pending() {
        PendingEntries::Absent => None,
        PendingEntries::Unioned => {
            let mut queued = Query::select();
            queued
                .expr_as(Expr::col((QueuedGrain, HistoryGrain::EntityId)), Entities::Id)
                .from_subquery(pending::stream_rows(branch.schema(), &HistoryOf::EveryEntity), QueuedGrain)
                .and_where(is_any_of(
                    Expr::col((QueuedGrain, HistoryGrain::AttributeNameKey)),
                    keys.iter().map(|key| key.get()),
                ))
                .order_by(Entities::Id, Order::Asc);
            let mut holders = Query::select();
            holders
                .column((QueuedHolder, Entities::Id))
                .expr_as(typed_null(PostgresType::Bigint), Entities::EntityKey)
                .from_subquery(queued.take(), QueuedHolder);
            Some(holders.take())
        }
    }
}

/// A walk of no holder: an attribute the dictionary never keyed was never
/// recorded, and one the census names no type for is held by no Evolution.
fn no_holder() -> SelectStatement {
    let mut none = Query::select();
    none.column(Entities::Id)
        .column(Entities::EntityKey)
        .from(Entities::Table)
        .and_where(false_literal());
    none.take()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_type_name_key::EntityTypeNameKey,
        history_source::{
            HistoryBranch,
            attribute_holders::{AttributeHolders, HolderType},
            entity_rows::WalkedKeys,
            holder_rows::{attribute_holder_rows, attribute_holder_rows_by_key},
        },
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Asterisk, IntoIden, PostgresQueryBuilder, Query, TableRef};

    fn rendered(source: TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    fn among_candidates() -> WalkedKeys {
        WalkedKeys::Among(Box::new(Query::select().column(Alias::new("entity_key")).from(Alias::new("candidates")).take()))
    }

    /// The census of the holder walks: `speed` (7) held by 20,000 streams
    /// each under types 3 and 4, `address` (9) by 12 under type 3, `colour`
    /// (11) by none.
    fn census() -> AttributeHolders {
        let holder = |type_key: i32, streams: u64| HolderType {
            type_key: EntityTypeNameKey::from(type_key),
            streams,
        };
        AttributeHolders::new([
            (AttributeNameKey::from(7), holder(3, 20_000)),
            (AttributeNameKey::from(7), holder(4, 20_000)),
            (AttributeNameKey::from(9), holder(3, 12)),
        ])
    }

    fn holders_among(branch: &HistoryBranch, keys: &[i32], walked: &WalkedKeys) -> String {
        let keys: Vec<AttributeNameKey> = keys.iter().copied().map(AttributeNameKey::from).collect();
        rendered(attribute_holder_rows(branch, Alias::new("e"), &keys, &census(), walked))
    }

    fn holders(branch: &HistoryBranch, keys: &[i32]) -> String {
        holders_among(branch, keys, &WalkedKeys::Every)
    }

    /// Clauses 5.7.4.4 S1 and 5.5.9: an attribute with many holders is walked
    /// through the Evolution memberships of each type holding it, in
    /// identifier order, keeping a member whose grain holds a stream of the
    /// attribute; the probe is a scalar subquery limited to one row.
    #[test]
    fn a_dense_attribute_walks_the_evolution_members_of_each_holding_type() {
        let sql = holders(&HistoryBranch::live_recorded(), &[7]);
        for type_key in [3, 4] {
            assert!(
                sql.contains(&format!(
                    concat!(
                        r#"(SELECT "hw"."id", "hw"."entity_key" FROM "entity_type_members" AS "hw" WHERE "hw"."entity_type_name_key" = ({}) AND "hw"."is_evolution_member" "#,
                        r#"AND (SELECT 1 FROM "temporal_attribute_streams" AS "hs" WHERE "hs"."entity_key" = "hw"."entity_key" AND "hs"."attribute_name_key" = (7) LIMIT 1) IS NOT NULL "#,
                        r#"ORDER BY "hw"."id" ASC)"#
                    ),
                    type_key
                )),
                "{sql}"
            );
        }
        assert!(
            sql.starts_with(r#"SELECT * FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."entity_key" FROM ("#),
            "{sql}"
        );
        assert!(sql.ends_with(r#") AS "held" ORDER BY "held"."id" ASC) AS "e""#), "{sql}");
    }

    /// An attribute whose census counts few streams is read whole from its
    /// holder index and sorted by identifier, which costs a page less than
    /// walking the members of its types.
    #[test]
    fn a_sparse_attribute_reads_its_holders_whole_and_sorts_them_by_identifier() {
        let sql = holders(&HistoryBranch::live_recorded(), &[9]);
        assert!(
            sql.contains(concat!(
                r#"(SELECT "hi"."id", "hs"."entity_key" FROM "temporal_attribute_streams" AS "hs" "#,
                r#"JOIN "entities" AS "hi" ON "hi"."entity_key" = "hs"."entity_key" WHERE "hs"."attribute_name_key" = (9) ORDER BY "hi"."id" ASC)"#
            )),
            "{sql}"
        );
        assert!(!sql.contains("entity_type_members"), "{sql}");
    }

    /// The ranges of several attributes merge in identifier order with each
    /// Evolution once, however many streams or types it holds.
    #[test]
    fn several_attributes_merge_their_ranges_with_each_holder_once() {
        let sql = holders(&HistoryBranch::live_recorded(), &[7, 9]);
        assert_eq!(sql.matches(r#"FROM "entity_type_members" AS "hw""#).count(), 2, "{sql}");
        assert_eq!(sql.matches(r#"JOIN "entities" AS "hi""#).count(), 1, "{sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
    }

    /// On the live branch an Entity whose queued entries extend a grain of the
    /// attribute that no recorded stream holds joins the merge, ordered on its
    /// own; an attribute the census names no type for has no recorded range.
    #[test]
    fn a_live_attribute_walk_merges_the_grains_the_queued_entries_extend() {
        let sql = holders(&HistoryBranch::live(), &[11]);
        assert!(
            sql.contains(r#"(SELECT "hq"."id", NULL::bigint AS "entity_key" FROM (SELECT "hg"."entity_id" AS "id" FROM (SELECT DISTINCT "pe"."entity_id""#),
            "{sql}"
        );
        assert!(sql.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
        assert!(
            sql.contains(r#") AS "hg" WHERE "hg"."attribute_name_key" = ANY(ARRAY [11]) ORDER BY "id" ASC) AS "hq""#),
            "{sql}"
        );
        assert!(!sql.contains(r#""temporal_attribute_streams" AS "hs""#), "{sql}");
    }

    #[test]
    fn a_snapshot_attribute_walk_reads_its_own_memberships_and_streams() {
        let sql = holders(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), &[7, 9]);
        assert!(sql.contains(r#"FROM "snap_1"."entity_type_members" AS "hw""#), "{sql}");
        assert!(sql.contains(r#"FROM "snap_1"."temporal_attribute_streams" AS "hs""#), "{sql}");
        assert!(sql.contains(r#"JOIN "snap_1"."entities" AS "hi""#), "{sql}");
        assert!(!sql.contains("temporal_append_entries"), "{sql}");
    }

    /// An attribute the dictionary never keyed was never recorded, and one the
    /// census names no type for is held by no Evolution, so neither walk
    /// yields a row.
    #[test]
    fn an_attribute_walk_of_no_key_or_no_holder_yields_no_row() {
        let none = r#"SELECT * FROM (SELECT "id", "entity_key" FROM "entities" WHERE FALSE) AS "e""#;
        pretty_assertions::assert_eq!(holders(&HistoryBranch::live(), &[]), none);
        pretty_assertions::assert_eq!(holders(&HistoryBranch::live_recorded(), &[11]), none);
        pretty_assertions::assert_eq!(rendered(attribute_holder_rows_by_key(&HistoryBranch::live(), Alias::new("e"), &[])), none);
    }

    /// For a count, each holder once, read from the holder index in the order
    /// of its surrogate key; a holder only queued has none and comes last.
    #[test]
    fn a_holder_walk_by_key_reads_the_holder_index_in_key_order() {
        let keys = [AttributeNameKey::from(7), AttributeNameKey::from(9)];
        let sql = rendered(attribute_holder_rows_by_key(&HistoryBranch::live(), Alias::new("e"), &keys));
        assert!(
            sql.starts_with(concat!(
                r#"SELECT * FROM (SELECT "hm"."id" FROM (SELECT DISTINCT ON ("hs"."entity_key") "hi"."id", "hs"."entity_key" FROM "temporal_attribute_streams" AS "hs" "#,
                r#"JOIN "entities" AS "hi" ON "hi"."entity_key" = "hs"."entity_key" WHERE "hs"."attribute_name_key" = ANY(ARRAY [7,9]) ORDER BY "hs"."entity_key" ASC) AS "hm""#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(r#"UNION ALL (SELECT "hq"."id", NULL::bigint AS "entity_key" FROM (SELECT "hg"."entity_id" AS "id""#),
            "{sql}"
        );
    }

    /// Narrowed, each range of a holder walk reads the named holders alone.
    #[test]
    fn a_narrowed_holder_walk_reads_the_named_holders_alone() {
        let sql = holders_among(&HistoryBranch::live_recorded(), &[7, 9], &among_candidates());
        assert_eq!(
            sql.matches(r#""hw"."entity_key" IN (SELECT "entity_key" FROM "candidates") ORDER BY "hw"."id" ASC"#)
                .count(),
            2,
            "{sql}"
        );
        assert!(
            sql.contains(r#""hs"."entity_key" IN (SELECT "entity_key" FROM "candidates") ORDER BY "hi"."id" ASC"#),
            "{sql}"
        );
    }
}
