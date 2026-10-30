//! Entities read back from their heads and their `current_attributes` rows,
//! and assembled into the states they store.
//!
//! A read gathers the heads and rows of every Entity it reads first, then looks
//! up the names of every key they hold, one lookup per dictionary for the whole
//! read, and only then assembles. Rows come in no order: each Entity's are put
//! in written order by their ordinal, and the Entities keep the order their
//! heads were read in.
//!
//! Each Entity is assembled on its own: one whose head past its key and
//! identifier, one of whose rows, or whose assembly does not read back is
//! named unreadable in its place, and the others assemble around it.

use crate::{
    current_attribute::{
        read::{RowSelection, row_of, rows_of},
        row::CurrentAttributeRow,
    },
    entity::{
        entity_types::Search,
        mapper::{stored_entity_key, stored_uri},
        repo::EntityRepository,
        stored_entity_error::StoredEntityError,
        unreadable_entity::UnreadableEntity,
    },
    entity_state_record::stored_state::StoredState,
    error::ContextStoreError,
};
use andromeda_context_commit::entity_version::EntityVersion;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, port::retrieved_entity::RetrievedEntity};
use andromeda_ngsi_ld_entity::scopes::Scopes;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, scope::path::ScopePath};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_postgresql_attribute_instance::{
    name_keys::{HeldKeys, KeyNames},
    name_resolution::names_of,
};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_context_query::entity_head::{EntityHead, select_head};
use andromeda_postgresql_pool::read_share::PoolLane;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_postgresql_schema::current::{CurrentAttributes, E, Entities, EntitySearch};
use sea_query::{Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};
use sqlx::{Row, postgres::PgRow};
use std::{collections::HashMap, sync::Arc};

/// An Entity's members beside its attributes, as read.
#[derive(Debug)]
pub struct StoredHead {
    /// The key the Entity's rows are read by.
    key: EntityKey,
    /// The identifier.
    id: Identifier,
    /// The type keys, in written order.
    types: Vec<EntityTypeNameKey>,
    /// The Scopes as written (clause 4.18), `None` when the Entity has none.
    scope: Option<Scopes>,
    /// The clause 4.8 and 4.22 instants.
    sys_attrs: SysAttrsSnapshot,
    /// The version the row stood at when it was read.
    version: EntityVersion,
}

impl StoredHead {
    /// The head a statement read through
    /// [`select_head`](andromeda_postgresql_context_query::entity_head::select_head),
    /// or the Entity it names when a column past its key and identifier does
    /// not read back.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the key or the identifier does not
    /// read back, without which the Entity cannot be named.
    pub(crate) fn of(row: &PgRow) -> Result<Result<StoredHead, UnreadableEntity>, ContextStoreError> {
        let key = stored_entity_key(row.try_get(EntityHead::EntityKey.unquoted())?)?;
        let id = stored_uri(row.try_get(EntityHead::Id.unquoted())?)?;
        Ok(match HeadColumns::of(row) {
            Ok(columns) => Ok(StoredHead {
                key,
                id,
                types: columns.types,
                scope: columns.scope,
                sys_attrs: columns.sys_attrs,
                version: EntityVersion::new(key.get(), columns.write_count),
            }),
            Err(fault) => Err(UnreadableEntity { id, fault: Box::new(fault) }),
        })
    }
}

/// The columns of a head beside its key and identifier.
struct HeadColumns {
    /// The type keys, in written order.
    types: Vec<EntityTypeNameKey>,
    /// The Scopes as written, `None` when the Entity has none.
    scope: Option<Scopes>,
    /// The clause 4.8 and 4.22 instants.
    sys_attrs: SysAttrsSnapshot,
    /// The number of writes the Entity's row has taken.
    write_count: i64,
}

impl HeadColumns {
    /// The columns of `row` beside its key and identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a column does not decode or a Scope
    /// does not read back.
    fn of(row: &PgRow) -> Result<HeadColumns, ContextStoreError> {
        // An entity without a Scope stores NULL rather than an empty list.
        let written: Option<Vec<String>> = row.try_get(EntityHead::VerbatimScopePaths.unquoted())?;
        let paths = written
            .into_iter()
            .flatten()
            .map(ScopePath::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoredEntityError::Scope)?;
        let types: Vec<i32> = row.try_get(EntityHead::EntityTypeNameKeys.unquoted())?;
        Ok(HeadColumns {
            types: types.into_iter().map(EntityTypeNameKey::from).collect(),
            scope: Scopes::try_from_paths(paths).ok(),
            sys_attrs: SysAttrsSnapshot {
                created_at: row.try_get(EntityHead::CreatedAt.unquoted())?,
                modified_at: row.try_get(EntityHead::ModifiedAt.unquoted())?,
                expires_at: row.try_get(EntityHead::ExpiresAt.unquoted())?,
            },
            write_count: row.try_get(EntityHead::WriteCount.unquoted())?,
        })
    }
}

/// An Entity as read, with the version its row stood at.
#[derive(Debug)]
pub(crate) struct VersionedEntity {
    /// The state and its system times.
    pub(crate) retrieved: RetrievedEntity,
    /// The version a write computed from this read states.
    pub(crate) version: EntityVersion,
}

impl VersionedEntity {
    /// The snapshot a read-modify-write states its precondition against.
    pub(crate) fn into_snapshot(self) -> EntitySnapshot {
        EntitySnapshot {
            state: self.retrieved.state,
            version: self.version,
        }
    }
}

/// The Entities a read gathered: heads in the order they were read, rows by
/// Entity.
#[derive(Debug, Default)]
pub struct GatheredEntities {
    /// The heads, in read order, each read back or naming its Entity
    /// unreadable.
    heads: Vec<Result<StoredHead, UnreadableEntity>>,
    /// The rows of each Entity that read back, in no order.
    rows: HashMap<EntityKey, Vec<CurrentAttributeRow>>,
    /// The first row of each Entity that did not read back, which makes the
    /// Entity unreadable.
    row_faults: HashMap<EntityKey, ContextStoreError>,
}

impl GatheredEntities {
    /// The heads a statement read through
    /// [`select_head`](andromeda_postgresql_context_query::entity_head::select_head),
    /// one per result row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a head's key or identifier does not
    /// read back.
    pub fn of_heads(rows: &[PgRow]) -> Result<GatheredEntities, ContextStoreError> {
        Ok(GatheredEntities {
            heads: rows.iter().map(StoredHead::of).collect::<Result<_, _>>()?,
            ..GatheredEntities::default()
        })
    }

    /// The heads and rows a statement read that left-joins each Entity's rows
    /// to its head: an Entity's head is read from the first result row that
    /// names it, and an Entity without rows contributes its head alone.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a head's key or identifier does not
    /// read back.
    pub fn of_joined(rows: &[PgRow]) -> Result<GatheredEntities, ContextStoreError> {
        let mut gathered = GatheredEntities::default();
        for row in rows {
            let key = stored_entity_key(row.try_get(EntityHead::EntityKey.unquoted())?)?;
            if !gathered.rows.contains_key(&key) {
                gathered.heads.push(StoredHead::of(row)?);
            }
            gathered.hold(key, row);
        }
        Ok(gathered)
    }

    /// Add the rows a statement read through
    /// [`rows_of`](crate::current_attribute::read::rows_of), each under its
    /// Entity's key.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a row's Entity key does not read
    /// back.
    pub fn add_rows(&mut self, rows: &[PgRow]) -> Result<(), ContextStoreError> {
        for row in rows {
            let key = stored_entity_key(row.try_get(CurrentAttributes::EntityKey.unquoted())?)?;
            self.hold(key, row);
        }
        Ok(())
    }

    /// Hold the attribute row `row` carries under the Entity `key`, or its
    /// fault when it does not read back.
    fn hold(&mut self, key: EntityKey, row: &PgRow) {
        let held = self.rows.entry(key).or_default();
        match row_of(row) {
            Ok(Some(read)) => held.push(read),
            Ok(None) => {}
            Err(fault) => {
                self.row_faults.entry(key).or_insert(fault);
            }
        }
    }

    /// The keys of the Entities whose heads read back, in the order they were
    /// read.
    #[must_use]
    pub fn keys(&self) -> Vec<EntityKey> {
        self.readable_heads().map(|head| head.key).collect()
    }

    /// The heads that read back, in read order.
    fn readable_heads(&self) -> impl Iterator<Item = &StoredHead> {
        self.heads.iter().filter_map(|head| head.as_ref().ok())
    }

    /// Every dictionary key the heads and rows hold, each once.
    fn held_keys(&self) -> HeldKeys {
        let mut held = HeldKeys::default();
        for row in self.rows.values().flatten() {
            held.collect(&row.members, &row.sub_attributes);
        }
        held.types.extend(self.readable_heads().flat_map(|head| head.types.iter().copied()));
        held.distinct()
    }

    /// The states of the gathered Entities with their versions, in the order
    /// their heads were read, every key read back through `names`; an Entity
    /// whose head or a row did not read back, one with a type key that has no
    /// name or no type at all, and one whose rows do not assemble are named
    /// unreadable in their place.
    fn assemble(mut self, names: &KeyNames) -> Vec<Result<VersionedEntity, UnreadableEntity>> {
        self.heads
            .into_iter()
            .map(|head| {
                let head = head?;
                let rows = self.rows.remove(&head.key).unwrap_or_default();
                if let Some(fault) = self.row_faults.remove(&head.key) {
                    return Err(UnreadableEntity {
                        id: head.id,
                        fault: Box::new(fault),
                    });
                }
                let stored = StoredState {
                    types: head.types,
                    scope: head.scope,
                    expires_at: head.sys_attrs.expires_at,
                    rows,
                };
                match stored.assemble(&head.id, names) {
                    Ok(state) => Ok(VersionedEntity {
                        retrieved: RetrievedEntity {
                            state: Arc::new(state),
                            sys_attrs: head.sys_attrs,
                        },
                        version: head.version,
                    }),
                    Err(fault) => Err(UnreadableEntity {
                        id: head.id,
                        fault: Box::new(fault.into()),
                    }),
                }
            })
            .collect()
    }
}

/// The heads of Entities, for a caller to filter, order and window:
///
/// ```sql
/// SELECT <head> FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key
/// ```
pub(crate) fn heads_statement(target: &ContextTarget) -> SelectStatement {
    let mut statement = Query::select();
    select_head(&mut statement, E, Search);
    statement.from_as(table_of(target, Entities::Table), E).join_as(
        JoinType::InnerJoin,
        table_of(target, EntitySearch::Table),
        Search,
        Expr::col((Search, EntitySearch::EntityKey)).equals((E, Entities::EntityKey)),
    );
    statement
}

impl EntityRepository {
    /// The Entities whose heads a statement read into `heads`, with the rows
    /// `selection` takes, in the order of their heads, each one that does not
    /// read back named unreadable in its place.
    ///
    /// The rows are a second statement on `lane`, by the keys of the heads; it
    /// is not sent when no head read back or the selection takes no row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the row read or a dictionary lookup
    /// fails, or when a head's key or identifier does not read back.
    pub(crate) async fn entities_of_heads(
        &self,
        lane: PoolLane,
        target: &ContextTarget,
        heads: &[PgRow],
        selection: &RowSelection,
    ) -> Result<Vec<Result<RetrievedEntity, UnreadableEntity>>, ContextStoreError> {
        Ok(retrieved(self.versioned_entities_of_heads(lane, target, heads, selection).await?))
    }

    /// [`entities_of_heads`](EntityRepository::entities_of_heads), each
    /// Entity with the version its row stood at.
    ///
    /// # Errors
    ///
    /// As [`entities_of_heads`](EntityRepository::entities_of_heads).
    pub(crate) async fn versioned_entities_of_heads(
        &self,
        lane: PoolLane,
        target: &ContextTarget,
        heads: &[PgRow],
        selection: &RowSelection,
    ) -> Result<Vec<Result<VersionedEntity, UnreadableEntity>>, ContextStoreError> {
        let mut gathered = GatheredEntities::of_heads(heads)?;
        let keys = gathered.keys();
        if !keys.is_empty() && !selection.takes_none() {
            let rows = self.fetch_on(lane, &rows_of(target, &keys, selection)).await?;
            gathered.add_rows(&rows)?;
        }
        self.versioned(gathered).await
    }

    /// The states of the Entities `gathered` holds, in the order their heads
    /// were read, the names of every key they hold looked up once, each one
    /// that does not read back named unreadable in its place.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a dictionary lookup fails.
    pub(crate) async fn assembled(&self, gathered: GatheredEntities) -> Result<Vec<Result<RetrievedEntity, UnreadableEntity>>, ContextStoreError> {
        Ok(retrieved(self.versioned(gathered).await?))
    }

    /// [`assembled`](EntityRepository::assembled), each Entity with the
    /// version its row stood at.
    ///
    /// # Errors
    ///
    /// As [`assembled`](EntityRepository::assembled).
    pub(crate) async fn versioned(&self, gathered: GatheredEntities) -> Result<Vec<Result<VersionedEntity, UnreadableEntity>>, ContextStoreError> {
        let names = names_of(&gathered.held_keys(), self.attribute_names.as_ref(), self.entity_type_names.as_ref()).await?;
        Ok(gathered.assemble(&names))
    }
}

/// The states of `entities`, their versions dropped.
fn retrieved(entities: Vec<Result<VersionedEntity, UnreadableEntity>>) -> Vec<Result<RetrievedEntity, UnreadableEntity>> {
    entities.into_iter().map(|entity| entity.map(|entity| entity.retrieved)).collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        current_attribute::{flatten::flatten, row::CurrentAttributeRow},
        entity::{
            stored_entities::{GatheredEntities, StoredHead, VersionedEntity},
            stored_entity_error::StoredEntityError,
            unreadable_entity::{UnreadableEntity, every_entity},
        },
        error::ContextStoreError,
    };
    use andromeda_context_commit::entity_version::EntityVersion;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        entity_type_name::EntityTypeName,
        reference::NgsiLdReference,
        time::system_instant::SystemInstant,
    };
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_postgresql_attribute_instance::name_keys::{KeyNames, NameKeys};
    use andromeda_postgresql_codes::entity_key::EntityKey;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::cmp::Reverse;

    const EX: &str = "https://ex.org/";

    fn property(value: i64) -> serde_json::Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    /// An Entity of type `Building` with the attributes `a`, `b` and `c` in
    /// that order, each holding `seed` plus its position.
    fn committed(id: &str, seed: i64) -> EntityState {
        let document = json!([{
            "@id": id,
            "@type": [format!("{EX}Building")],
            format!("{EX}a"): property(seed),
            format!("{EX}b"): property(seed + 1),
            format!("{EX}c"): property(seed + 2),
        }]);
        let mut state = EntityState::from_slice(document.to_string().as_bytes()).expect("a stored entity");
        state.settle(
            None,
            SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).single().expect("an instant")),
        );
        state
    }

    /// The keys of `a`, `b` and `c`, and the names behind them and behind
    /// the `Building` type key 9.
    fn names() -> (NameKeys, KeyNames) {
        let attributes: Vec<(AttributeName, AttributeNameKey)> = ["a", "b", "c"]
            .iter()
            .zip(1..)
            .map(|(name, key)| (format!("{EX}{name}").parse().expect("a name"), AttributeNameKey::from(key)))
            .collect();
        let building: EntityTypeName = format!("{EX}Building").parse().expect("a type");
        let keys = NameKeys::new(ResolvedNames::from_pairs(attributes.clone()), ResolvedNames::default());
        let names = KeyNames::from_pairs(
            attributes.into_iter().map(|(name, key)| (key, name)),
            [(EntityTypeNameKey::from(9), NgsiLdReference::from(&building))],
        );
        (keys, names)
    }

    fn head(key: i64, id: &str) -> StoredHead {
        let instant = Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).single().expect("an instant");
        StoredHead {
            key: EntityKey::try_from(key).expect("a surrogate key"),
            id: id.parse().expect("an identifier"),
            types: vec![EntityTypeNameKey::from(9)],
            scope: None,
            sys_attrs: SysAttrsSnapshot {
                created_at: instant,
                modified_at: instant,
                expires_at: None,
            },
            version: EntityVersion::new(key, 1),
        }
    }

    /// Stage one fixes the order of the page and stage two reads rows in no
    /// order: the Entities come back in head order, each one's attributes in
    /// ordinal order, whatever order the rows arrived in.
    #[test]
    fn rows_of_a_page_in_any_order_assemble_in_page_order() {
        let (keys, names) = names();
        let page = [
            (3, "urn:ngsi-ld:Building:c", 30),
            (1, "urn:ngsi-ld:Building:a", 10),
            (2, "urn:ngsi-ld:Building:b", 20),
        ];
        let mut gathered = GatheredEntities::default();
        let mut interleaved: Vec<(EntityKey, CurrentAttributeRow)> = Vec::new();
        for (key, id, seed) in page {
            gathered.heads.push(Ok(head(key, id)));
            let rows = flatten(committed(id, seed).attributes(), &keys).expect("the state flattens");
            interleaved.extend(rows.into_iter().map(|row| (EntityKey::try_from(key).expect("a surrogate key"), row)));
        }
        interleaved.sort_by_key(|(key, row)| (Reverse(row.ordinal), key.get()));
        for (key, row) in interleaved {
            gathered.rows.entry(key).or_default().push(row);
        }

        let assembled = every_entity(gathered.assemble(&names)).expect("the rows assemble");
        let ids: Vec<&str> = assembled.iter().map(|entity| entity.retrieved.state.id().as_str()).collect();
        assert_eq!(ids, vec!["urn:ngsi-ld:Building:c", "urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:b"]);
        for ((key, id, seed), entity) in page.iter().zip(&assembled) {
            assert_eq!(entity.retrieved.state.as_ref(), &committed(id, *seed));
            assert_eq!(entity.version, EntityVersion::new(*key, 1));
        }
    }

    /// An Entity whose every attribute was removed still reads back, with no
    /// attribute.
    #[test]
    fn an_entity_without_rows_assembles_without_attributes() {
        let (_, names) = names();
        let mut gathered = GatheredEntities::default();
        gathered.heads.push(Ok(head(1, "urn:ngsi-ld:Building:a")));
        let assembled = every_entity(gathered.assemble(&names)).expect("the head assembles");
        assert!(assembled.first().is_some_and(|entity| entity.retrieved.state.attributes().is_empty()));
    }

    /// A type key the dictionary has no name for is a store fault, never an
    /// Entity without that type.
    #[test]
    fn a_type_key_without_a_name_is_refused() {
        let mut gathered = GatheredEntities::default();
        gathered.heads.push(Ok(head(1, "urn:ngsi-ld:Building:a")));
        assert!(matches!(
            gathered.assemble(&KeyNames::default()).as_slice(),
            [Err(UnreadableEntity { fault, .. })] if matches!(**fault, ContextStoreError::StoredEntity(StoredEntityError::TypeKey(_)))
        ));
    }

    /// What each assembled entry is: the identifier of an Entity that read
    /// back, or of one named unreadable.
    fn outcome(entry: &Result<VersionedEntity, UnreadableEntity>) -> Result<&str, &str> {
        entry
            .as_ref()
            .map(|entity| entity.retrieved.state.id().as_str())
            .map_err(|unreadable| unreadable.id.as_str())
    }

    /// One Entity that does not assemble is named unreadable in its place, and
    /// the Entities before and after it still read back.
    #[test]
    fn an_entity_that_does_not_assemble_leaves_its_neighbours_readable() {
        let (_, names) = names();
        let mut gathered = GatheredEntities::default();
        let mut untyped = head(2, "urn:ngsi-ld:Building:b");
        untyped.types = vec![EntityTypeNameKey::from(99)];
        gathered
            .heads
            .extend([Ok(head(1, "urn:ngsi-ld:Building:a")), Ok(untyped), Ok(head(3, "urn:ngsi-ld:Building:c"))]);
        let assembled = gathered.assemble(&names);
        let outcomes: Vec<_> = assembled.iter().map(outcome).collect();
        assert_eq!(
            outcomes,
            vec![Ok("urn:ngsi-ld:Building:a"), Err("urn:ngsi-ld:Building:b"), Ok("urn:ngsi-ld:Building:c")]
        );
    }

    /// A row that did not read back makes only its own Entity unreadable,
    /// with that row's fault.
    #[test]
    fn a_row_that_does_not_read_back_makes_only_its_entity_unreadable() {
        let (keys, names) = names();
        let mut gathered = GatheredEntities::default();
        for (key, id) in [(1, "urn:ngsi-ld:Building:a"), (2, "urn:ngsi-ld:Building:b")] {
            gathered.heads.push(Ok(head(key, id)));
            let rows = flatten(committed(id, 10).attributes(), &keys).expect("the state flattens");
            gathered.rows.insert(EntityKey::try_from(key).expect("a surrogate key"), rows);
        }
        gathered
            .row_faults
            .insert(EntityKey::try_from(2).expect("a surrogate key"), StoredEntityError::Untyped.into());
        let assembled = gathered.assemble(&names);
        assert!(matches!(
            assembled.as_slice(),
            [Ok(_), Err(UnreadableEntity { id, fault })]
                if id.as_str() == "urn:ngsi-ld:Building:b" && matches!(**fault, ContextStoreError::StoredEntity(StoredEntityError::Untyped))
        ));
        assert_eq!(outcome(&assembled[0]), Ok("urn:ngsi-ld:Building:a"));
    }

    /// A head that did not read back keeps its place among the heads, and its
    /// rows are not asked for.
    #[test]
    fn an_unreadable_head_keeps_its_place_and_asks_for_no_rows() {
        let (_, names) = names();
        let mut gathered = GatheredEntities::default();
        gathered.heads.extend([
            Ok(head(1, "urn:ngsi-ld:Building:a")),
            Err(UnreadableEntity {
                id: "urn:ngsi-ld:Building:b".parse().expect("an identifier"),
                fault: Box::new(StoredEntityError::Untyped.into()),
            }),
            Ok(head(3, "urn:ngsi-ld:Building:c")),
        ]);
        assert_eq!(
            gathered.keys(),
            vec![
                EntityKey::try_from(1).expect("a surrogate key"),
                EntityKey::try_from(3).expect("a surrogate key")
            ]
        );
        let assembled = gathered.assemble(&names);
        let outcomes: Vec<_> = assembled.iter().map(outcome).collect();
        assert_eq!(
            outcomes,
            vec![Ok("urn:ngsi-ld:Building:a"), Err("urn:ngsi-ld:Building:b"), Ok("urn:ngsi-ld:Building:c")]
        );
    }
}
