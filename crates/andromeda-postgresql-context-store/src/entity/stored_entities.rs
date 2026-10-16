//! Entities read back from their heads and their `current_attributes` rows,
//! and assembled into the states they store.
//!
//! A read gathers the heads and rows of every Entity it reads first, then looks
//! up the names of every key they hold, one lookup per dictionary for the whole
//! read, and only then assembles. Rows come in no order: each Entity's are put
//! in written order by their ordinal, and the Entities keep the order their
//! heads were read in.

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
    },
    entity_state_record::stored_state::StoredState,
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::retrieved_entity::RetrievedEntity;
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
}

impl StoredHead {
    /// The head a statement read through
    /// [`select_head`](andromeda_postgresql_context_query::entity_head::select_head).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a column does not decode, or when the
    /// key, the identifier or a Scope does not read back.
    pub fn of(row: &PgRow) -> Result<StoredHead, ContextStoreError> {
        let written: Vec<String> = row.try_get(EntityHead::VerbatimScopePaths.unquoted())?;
        let paths = written
            .into_iter()
            .map(ScopePath::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoredEntityError::Scope)?;
        let types: Vec<i32> = row.try_get(EntityHead::EntityTypeNameKeys.unquoted())?;
        Ok(StoredHead {
            key: stored_entity_key(row.try_get(EntityHead::EntityKey.unquoted())?)?,
            id: stored_uri(row.try_get(EntityHead::Id.unquoted())?)?,
            types: types.into_iter().map(EntityTypeNameKey::from).collect(),
            scope: Scopes::try_from_paths(paths).ok(),
            sys_attrs: SysAttrsSnapshot {
                created_at: row.try_get(EntityHead::CreatedAt.unquoted())?,
                modified_at: row.try_get(EntityHead::ModifiedAt.unquoted())?,
                expires_at: row.try_get(EntityHead::ExpiresAt.unquoted())?,
            },
        })
    }
}

/// The Entities a read gathered: heads in the order they were read, rows by
/// Entity.
#[derive(Debug, Default)]
pub struct GatheredEntities {
    /// The heads, in read order.
    heads: Vec<StoredHead>,
    /// The rows of each Entity, in no order.
    rows: HashMap<EntityKey, Vec<CurrentAttributeRow>>,
}

impl GatheredEntities {
    /// The heads a statement read through
    /// [`select_head`](andromeda_postgresql_context_query::entity_head::select_head),
    /// one per result row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a head does not read back.
    pub fn of_heads(rows: &[PgRow]) -> Result<GatheredEntities, ContextStoreError> {
        Ok(GatheredEntities {
            heads: rows.iter().map(StoredHead::of).collect::<Result<_, _>>()?,
            rows: HashMap::new(),
        })
    }

    /// The heads and rows a statement read that left-joins each Entity's rows
    /// to its head: an Entity's head is read from the first result row that
    /// names it, and an Entity without rows contributes its head alone.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a head or a row does not read back.
    pub fn of_joined(rows: &[PgRow]) -> Result<GatheredEntities, ContextStoreError> {
        let mut gathered = GatheredEntities::default();
        for row in rows {
            let key = stored_entity_key(row.try_get(EntityHead::EntityKey.unquoted())?)?;
            if !gathered.rows.contains_key(&key) {
                gathered.heads.push(StoredHead::of(row)?);
            }
            let held = gathered.rows.entry(key).or_default();
            if let Some(row) = row_of(row)? {
                held.push(row);
            }
        }
        Ok(gathered)
    }

    /// Add the rows a statement read through
    /// [`rows_of`](crate::current_attribute::read::rows_of), each under its
    /// Entity's key.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a row does not read back.
    pub fn add_rows(&mut self, rows: &[PgRow]) -> Result<(), ContextStoreError> {
        for row in rows {
            let key = stored_entity_key(row.try_get(CurrentAttributes::EntityKey.unquoted())?)?;
            if let Some(row) = row_of(row)? {
                self.rows.entry(key).or_default().push(row);
            }
        }
        Ok(())
    }

    /// The keys of the Entities gathered, in the order their heads were read.
    #[must_use]
    pub fn keys(&self) -> Vec<EntityKey> {
        self.heads.iter().map(|head| head.key).collect()
    }

    /// Whether no Entity was gathered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.heads.is_empty()
    }

    /// Every dictionary key the heads and rows hold, each once.
    fn held_keys(&self) -> HeldKeys {
        let mut held = HeldKeys::default();
        for row in self.rows.values().flatten() {
            held.collect(&row.members, &row.sub_attributes);
        }
        held.types.extend(self.heads.iter().flat_map(|head| head.types.iter().copied()));
        held.distinct()
    }

    /// The states of the gathered Entities, in the order their heads were
    /// read, every key read back through `names`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::StoredEntity`] when a type key has no name,
    /// when an Entity carries no type, or when its rows do not assemble.
    fn assemble(mut self, names: &KeyNames) -> Result<Vec<RetrievedEntity>, ContextStoreError> {
        self.heads
            .into_iter()
            .map(|head| {
                let stored = StoredState {
                    types: head.types,
                    scope: head.scope,
                    expires_at: head.sys_attrs.expires_at,
                    rows: self.rows.remove(&head.key).unwrap_or_default(),
                };
                Ok(RetrievedEntity {
                    state: Arc::new(stored.assemble(head.id, names)?),
                    sys_attrs: head.sys_attrs,
                })
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
    /// `selection` takes, in the order of their heads.
    ///
    /// The rows are a second statement on `lane`, by the keys of the heads; it
    /// is not sent when there is no head or the selection takes no row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the row read or a dictionary lookup
    /// fails, or when a head or the rows of an Entity do not read back.
    pub(crate) async fn entities_of_heads(
        &self,
        lane: PoolLane,
        target: &ContextTarget,
        heads: &[PgRow],
        selection: &RowSelection,
    ) -> Result<Vec<RetrievedEntity>, ContextStoreError> {
        let mut gathered = GatheredEntities::of_heads(heads)?;
        if !gathered.is_empty() && !selection.takes_none() {
            let rows = self.fetch_on(lane, &rows_of(target, &gathered.keys(), selection)).await?;
            gathered.add_rows(&rows)?;
        }
        self.assembled(gathered).await
    }

    /// The states of the Entities `gathered` holds, in the order their heads
    /// were read, the names of every key they hold looked up once.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a dictionary lookup fails, or when a
    /// head or the rows of an Entity do not read back.
    pub(crate) async fn assembled(&self, gathered: GatheredEntities) -> Result<Vec<RetrievedEntity>, ContextStoreError> {
        let names = names_of(&gathered.held_keys(), self.attribute_names.as_ref(), self.entity_type_names.as_ref()).await?;
        gathered.assemble(&names)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute::{flatten::flatten, row::CurrentAttributeRow},
        entity::stored_entities::{GatheredEntities, StoredHead},
    };
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, system_instant::SystemInstant};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReference};
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
            gathered.heads.push(head(key, id));
            let rows = flatten(committed(id, seed).attributes(), &keys).expect("the state flattens");
            interleaved.extend(rows.into_iter().map(|row| (EntityKey::try_from(key).expect("a surrogate key"), row)));
        }
        interleaved.sort_by_key(|(key, row)| (Reverse(row.ordinal), key.get()));
        for (key, row) in interleaved {
            gathered.rows.entry(key).or_default().push(row);
        }

        let assembled = gathered.assemble(&names).expect("the rows assemble");
        let ids: Vec<&str> = assembled.iter().map(|entity| entity.state.id().as_str()).collect();
        assert_eq!(ids, vec!["urn:ngsi-ld:Building:c", "urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:b"]);
        for ((_, id, seed), entity) in page.iter().zip(&assembled) {
            assert_eq!(entity.state.as_ref(), &committed(id, *seed));
        }
    }

    /// An Entity whose every attribute was removed still reads back, with no
    /// attribute.
    #[test]
    fn an_entity_without_rows_assembles_without_attributes() {
        let (_, names) = names();
        let mut gathered = GatheredEntities::default();
        gathered.heads.push(head(1, "urn:ngsi-ld:Building:a"));
        let assembled = gathered.assemble(&names).expect("the head assembles");
        assert!(assembled.first().is_some_and(|entity| entity.state.attributes().is_empty()));
    }

    /// A type key the dictionary has no name for is a store fault, never an
    /// Entity without that type.
    #[test]
    fn a_type_key_without_a_name_is_refused() {
        let mut gathered = GatheredEntities::default();
        gathered.heads.push(head(1, "urn:ngsi-ld:Building:a"));
        assert!(gathered.assemble(&KeyNames::default()).is_err());
    }
}
