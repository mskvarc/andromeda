//! PostgreSQL source of the entity locations an entity spatial index is built
//! and caught up from.
//!
//! Every read returns, in the one statement that reads the rows, the `xmax` of
//! that statement's snapshot and the transactions still in progress below it.
//! Every transaction below `xmax` and not in progress had committed or aborted
//! when the snapshot was taken, so a row it wrote is either in the read or was
//! overwritten by a later one. Every row the read could not see carries a
//! `written_xid` at or above `xmax` or among those in progress. Reading the rows
//! written at or above the previous read's `xmax`, or by one of its in-progress
//! transactions, therefore misses no change the previous read did not already
//! hold, whichever broker process committed it, and reads none it held. A
//! transaction left open somewhere in the cluster stays in the list and costs
//! one index probe, rather than holding a lower bound back.
//!
//! A change read also stops below the `xmax` of its own snapshot. Every row the
//! snapshot sees was written by a transaction below that `xmax`, so the bound
//! drops no change. It drops a row restored from a dump of another cluster,
//! whose `written_xid` may lie ahead of this cluster's counter. The full read
//! that seeds the index holds such a row already, and without the bound every
//! change read would return it again until the counter passed it. A later
//! write restamps the row with this cluster's identifier.

use crate::{
    entity::{
        mapper::{stored_type_name, stored_uri},
        repo::EntityRepository,
    },
    store_fault::{contract_broken, persistence_failure, undecodable},
};
use andromeda_entity::{
    error::EntityError,
    port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::{cast, typed_null},
    pg_array::any,
    pg_type::PostgresType,
    setting::lift_statement_timeout,
    statement::Statement,
    transaction_id::{as_position, as_transaction_id, as_transaction_ids, snapshot_in_progress_positions, snapshot_xmax},
};
use andromeda_postgresql_stored_form::names_in_key_order::type_names_of;
use async_trait::async_trait;
use sea_query::{Condition, Expr, ExprTrait, JoinType, Query, SelectStatement, UnionType};
use std::num::NonZeroUsize;
use tracing::warn;

/// A [`ChangeHorizon`] in the `bigint` form the statement binds it as.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundHorizon {
    /// The previous read's `xmax`.
    ceiling: i64,
    /// The transactions in progress below it when the previous read was taken.
    unseen: Vec<i64>,
}

impl TryFrom<&ChangeHorizon> for BoundHorizon {
    type Error = EntityError;

    fn try_from(horizon: &ChangeHorizon) -> Result<BoundHorizon, EntityError> {
        let position = |position: u64| i64::try_from(position).map_err(|error| EntityError::from(PersistenceFailure::Unencodable.record(&error)));
        Ok(BoundHorizon {
            ceiling: position(horizon.ceiling())?,
            unseen: horizon.unseen().iter().copied().map(position).collect::<Result<_, _>>()?,
        })
    }
}

/// How many changed entities a change read returns.
#[derive(Clone, Copy, Debug)]
enum ChangeBound {
    /// Every one, however long the read takes.
    Every,
    /// At most this many, and one more to tell that more changed.
    AtMost(NonZeroUsize),
}

/// Which entities a location read returns.
#[derive(Clone, Copy, Debug)]
enum LocationFilter<'h> {
    /// Every entity that conveys a `location`.
    Located,
    /// Every entity whose search row was written above this horizon and below
    /// the `xmax` of the read's snapshot, located or not, since an entity that
    /// lost its `location` has to leave the index.
    ChangedSince(&'h BoundHorizon, ChangeBound),
}

/// One row of a location read. The horizon row carries only the horizon, and
/// every entity row carries everything else.
type LocationRow = (Option<i64>, Option<Vec<i64>>, Option<String>, Option<Vec<String>>, Option<Vec<u8>>);

/// The horizon of the statement's snapshot, then one row per entity the filter
/// selects: its identifier, its type IRIs (read back through the type
/// dictionary) and its `location` as EWKB.
///
/// The two are one `UNION ALL` so they share the statement's one snapshot, and
/// the horizon row comes back even when no entity does.
fn locations_statement(filter: LocationFilter<'_>) -> SelectStatement {
    let mut entities = Query::select();
    entities
        .expr(typed_null(PostgresType::Bigint))
        .expr(typed_null(PostgresType::BigintArray))
        .column((Entities::Table, Entities::Id))
        .expr(type_names_of(Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys))))
        .expr(cast(Expr::col((EntitySearch::Table, EntitySearch::Location)), PostgresType::Bytea))
        .from(Entities::Table)
        .join(
            JoinType::Join,
            EntitySearch::Table,
            Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        );
    match filter {
        LocationFilter::Located => {
            entities.and_where(Expr::col((EntitySearch::Table, EntitySearch::Location)).is_not_null());
        }
        LocationFilter::ChangedSince(horizon, bound) => {
            entities
                .cond_where(changed_above(horizon))
                .and_where(Expr::col((EntitySearch::Table, EntitySearch::WrittenXid)).lt(snapshot_xmax()));
            match bound {
                ChangeBound::Every => {}
                ChangeBound::AtMost(at_most) => {
                    entities.limit(u64::try_from(at_most.get()).map_or(u64::MAX, |at_most| at_most.saturating_add(1)));
                }
            }
        }
    }
    Query::select()
        .expr(as_position(snapshot_xmax()))
        .expr(snapshot_in_progress_positions())
        .expr(typed_null(PostgresType::Text))
        .expr(typed_null(PostgresType::TextArray))
        .expr(typed_null(PostgresType::Bytea))
        .union(UnionType::All, entities.take())
        .to_owned()
}

/// Rows written at or above the horizon's ceiling, or by one of the
/// transactions in progress below it. Both arms are served by the index on
/// `written_xid`.
fn changed_above(horizon: &BoundHorizon) -> Condition {
    let written = || Expr::col((EntitySearch::Table, EntitySearch::WrittenXid));
    let condition = Condition::any().add(written().gte(as_transaction_id(Expr::val(horizon.ceiling))));
    if horizon.unseen.is_empty() {
        condition
    } else {
        // clone: the statement owns every value it binds
        condition.add(written().eq(any(as_transaction_ids(Expr::val(horizon.unseen.clone())))))
    }
}

impl EntityRepository {
    /// Run one location read, in a transaction of its own.
    ///
    /// A read of every location seeds or rebuilds a tenant's index, and an
    /// unbounded change read takes in a backlog. Both take as long as what
    /// they read is large and no request waits for either, so they lift the
    /// statement timeout for their own transaction. A bounded change read
    /// keeps it.
    async fn location_rows(&self, filter: LocationFilter<'_>) -> Result<Vec<LocationRow>, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        match filter {
            LocationFilter::Located | LocationFilter::ChangedSince(_, ChangeBound::Every) => {
                Query::select().expr(lift_statement_timeout()).to_query().execute(&mut *transaction).await?;
            }
            LocationFilter::ChangedSince(_, ChangeBound::AtMost(_)) => {}
        }
        let rows = locations_statement(filter).to_query_as::<LocationRow>().fetch_all(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(rows)
    }

    /// Run one location read and decode its rows.
    async fn read_locations(&self, filter: LocationFilter<'_>) -> Result<LocationRead, EntityError> {
        let rows = self
            .location_rows(filter)
            .await
            .map_err(|error| EntityError::from(persistence_failure(&error)))?;
        decode_rows(rows)
    }
}

/// Decode a location read's rows into the read.
fn decode_rows(rows: Vec<LocationRow>) -> Result<LocationRead, EntityError> {
    let mut horizon = None;
    let mut locations = Vec::with_capacity(rows.len());
    for row in rows {
        match row {
            (Some(ceiling), Some(unseen), None, None, None) => {
                horizon = Some(decode_horizon(ceiling, unseen)?);
            }
            (None, None, Some(identifier), Some(entity_types), location) => {
                if let Some(location) = decode_location(&identifier, &entity_types, location)? {
                    locations.push(location);
                }
            }
            _malformed => {
                return Err(EntityError::from(contract_broken()));
            }
        }
    }
    let horizon = horizon.ok_or_else(|| EntityError::from(contract_broken()))?;
    Ok(LocationRead { locations, horizon })
}

/// Decode the horizon row's `xmax` and in-progress transactions.
fn decode_horizon(ceiling: i64, unseen: Vec<i64>) -> Result<ChangeHorizon, EntityError> {
    let position = |position: i64| u64::try_from(position).map_err(|error| EntityError::from(undecodable(&error)));
    Ok(ChangeHorizon::new(
        position(ceiling)?,
        unseen.into_iter().map(position).collect::<Result<_, _>>()?,
    ))
}

/// Decode a change read bounded to `at_most` entities, or report that more
/// changed.
///
/// Counted over the entity rows the read returned before any is decoded, since
/// the decode skips a row whose stored form no entity could have been written
/// with.
fn decode_bounded_rows(rows: Vec<LocationRow>, at_most: NonZeroUsize) -> Result<BoundedChangeRead, EntityError> {
    if rows.iter().filter(|(_, _, identifier, _, _)| identifier.is_some()).count() > at_most.get() {
        return Ok(BoundedChangeRead::Exceeded);
    }
    decode_rows(rows).map(BoundedChangeRead::Within)
}

/// Decode one entity row, or `None` for a row whose identifier or geometry the
/// store holds in a form no entity could have been written with.
fn decode_location(identifier: &str, entity_types: &[String], location: Option<Vec<u8>>) -> Result<Option<EntityLocation>, EntityError> {
    let entity_id = match stored_uri(identifier) {
        Ok(entity_id) => entity_id,
        Err(error) => {
            warn!(entity = %identifier, %error, "spatial location read skipped an invalid stored entity identifier");
            return Ok(None);
        }
    };
    let location = match location.map(|bytes| Ewkb::try_from(bytes.as_slice()).and_then(|ewkb| Geometry::try_from(&ewkb))) {
        None => None,
        Some(Ok(geometry)) => Some(geometry),
        Some(Err(error)) => {
            warn!(entity = %identifier, %error, "spatial location read skipped an undecodable location");
            return Ok(None);
        }
    };
    let entity_types = entity_types
        .iter()
        .map(String::as_str)
        .map(stored_type_name)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EntityError::from(persistence_failure(&error)))?;
    Ok(Some(EntityLocation {
        entity_id,
        entity_types,
        location,
    }))
}

#[async_trait]
impl EntitySpatialSnapshotSource for EntityRepository {
    async fn locations(&self) -> Result<LocationRead, EntityError> {
        self.read_locations(LocationFilter::Located).await
    }

    async fn locations_changed_since(&self, horizon: &ChangeHorizon) -> Result<LocationRead, EntityError> {
        let horizon = BoundHorizon::try_from(horizon)?;
        self.read_locations(LocationFilter::ChangedSince(&horizon, ChangeBound::Every)).await
    }

    async fn locations_changed_within(&self, horizon: &ChangeHorizon, bound: NonZeroUsize) -> Result<BoundedChangeRead, EntityError> {
        let horizon = BoundHorizon::try_from(horizon)?;
        let rows = self
            .location_rows(LocationFilter::ChangedSince(&horizon, ChangeBound::AtMost(bound)))
            .await
            .map_err(|error| EntityError::from(persistence_failure(&error)))?;
        decode_bounded_rows(rows, bound)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        port::spatial_snapshot::{BoundHorizon, ChangeBound, LocationFilter, LocationRow, decode_bounded_rows, decode_rows, locations_statement},
        repo::EntityRepository,
    };
    use andromeda_entity::{
        error::EntityError,
        port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntitySpatialSnapshotSource},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroUsize;

    fn assert_spatial_source<T: EntitySpatialSnapshotSource>() {}

    /// The dictionary lookup every read projects the type IRIs through.
    const TYPE_NAMES: &str = r#"ARRAY (SELECT "entity_type_names"."name" FROM "entity_type_names" WHERE "entity_type_names"."entity_type_name_key" = ANY("entity_search"."entity_type_name_keys") ORDER BY array_position("entity_search"."entity_type_name_keys", "entity_type_names"."entity_type_name_key") ASC)"#;

    /// The horizon row every read starts with.
    const HORIZON: &str = "SELECT pg_snapshot_xmax(pg_current_snapshot())::text::bigint, ARRAY (SELECT pg_snapshot_xip(pg_current_snapshot())::text::bigint), NULL::text, NULL::text[], NULL::bytea";

    /// The head of every entity row.
    const ENTITIES: &str = r#"SELECT NULL::bigint, NULL::bigint[], "entities"."id", {TYPE_NAMES}, "entity_search"."location"::bytea FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key""#;

    fn entities() -> String {
        ENTITIES.replace("{TYPE_NAMES}", TYPE_NAMES)
    }

    /// An entity row naming `id` as a `Vehicle` with no location.
    fn vehicle_row(id: &str) -> LocationRow {
        (None, None, Some(id.to_owned()), Some(vec!["https://example.org/Vehicle".to_owned()]), None)
    }

    #[test]
    fn entity_repository_directly_implements_the_spatial_snapshot_source() {
        assert_spatial_source::<EntityRepository>();
    }

    #[test]
    fn a_full_read_returns_the_snapshot_ceiling_and_in_progress_transactions_beside_every_located_entity() {
        let (sql, values) = locations_statement(LocationFilter::Located).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(r#"{HORIZON} UNION ALL ({} WHERE "entity_search"."location" IS NOT NULL)"#, entities()),
            "{sql}"
        );
        assert_eq!(values.0.len(), 0, "the full read binds nothing: {sql}");
    }

    #[test]
    fn a_bounded_change_read_selects_above_the_ceiling_or_among_the_unseen_and_stops_one_past_its_bound() {
        let horizon = BoundHorizon {
            ceiling: 42,
            unseen: vec![17, 40],
        };
        let bound = NonZeroUsize::new(10_000).expect("a positive bound");
        let (sql, values) = locations_statement(LocationFilter::ChangedSince(&horizon, ChangeBound::AtMost(bound))).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"{HORIZON} UNION ALL ({} WHERE ("entity_search"."written_xid" >= ($1::text::xid8) OR "entity_search"."written_xid" = ANY($2::text[]::xid8[])) AND "entity_search"."written_xid" < pg_snapshot_xmax(pg_current_snapshot()) LIMIT $3)"#,
                entities()
            ),
            "{sql}"
        );
        assert_eq!(
            values.0.len(),
            3,
            "the bounded read binds its ceiling, its unseen transactions and its limit: {sql}"
        );
    }

    #[test]
    fn a_change_read_with_nothing_in_progress_selects_above_the_ceiling_alone() {
        let horizon = BoundHorizon {
            ceiling: 42,
            unseen: Vec::new(),
        };
        let (sql, values) = locations_statement(LocationFilter::ChangedSince(&horizon, ChangeBound::Every)).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"{HORIZON} UNION ALL ({} WHERE "entity_search"."written_xid" >= ($1::text::xid8) AND "entity_search"."written_xid" < pg_snapshot_xmax(pg_current_snapshot()))"#,
                entities()
            ),
            "{sql}"
        );
        assert_eq!(values.0.len(), 1, "the unbounded read binds its ceiling alone: {sql}");
    }

    #[test]
    fn a_horizon_beyond_the_signed_range_is_unencodable() {
        assert!(matches!(
            BoundHorizon::try_from(&ChangeHorizon::new(u64::MAX, Vec::new())),
            Err(EntityError::Persistence(PersistenceFailure::Unencodable))
        ));
    }

    #[test]
    fn decoded_rows_give_the_horizon_and_every_location() {
        let read = decode_rows(vec![
            vehicle_row("urn:ngsi-ld:Vehicle:a"),
            (Some(9), Some(vec![4, 7]), None, None, None),
            vehicle_row("urn:ngsi-ld:Vehicle:b"),
        ])
        .expect("well-formed rows");
        assert_eq!(read.horizon, ChangeHorizon::new(9, vec![4, 7]));
        assert_eq!(
            read.locations.iter().map(|location| location.entity_id.clone()).collect::<Vec<Identifier>>(),
            vec![
                "urn:ngsi-ld:Vehicle:a".parse::<Identifier>().expect("a well-formed URI"),
                "urn:ngsi-ld:Vehicle:b".parse().expect("a well-formed URI"),
            ]
        );
    }

    #[test]
    fn a_bounded_read_one_past_its_bound_is_exceeded() {
        let rows = vec![
            (Some(9), Some(Vec::new()), None, None, None),
            vehicle_row("urn:ngsi-ld:Vehicle:a"),
            vehicle_row("urn:ngsi-ld:Vehicle:b"),
            vehicle_row("urn:ngsi-ld:Vehicle:c"),
        ];
        let two = NonZeroUsize::new(2).expect("a positive bound");
        assert!(matches!(decode_bounded_rows(rows.clone(), two), Ok(BoundedChangeRead::Exceeded)));
        let three = NonZeroUsize::new(3).expect("a positive bound");
        assert!(matches!(decode_bounded_rows(rows, three), Ok(BoundedChangeRead::Within(read)) if read.locations.len() == 3));
    }

    #[test]
    fn a_read_without_its_horizon_row_breaks_the_contract() {
        assert!(matches!(
            decode_rows(vec![vehicle_row("urn:ngsi-ld:Vehicle:a")]),
            Err(EntityError::Persistence(PersistenceFailure::ContractBroken))
        ));
    }
}
