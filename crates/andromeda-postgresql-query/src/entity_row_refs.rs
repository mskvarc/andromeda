//! How a statement reaches the two relations that hold one entity's identity
//! and its indexed query axes.
//!
//! An entity is an `entities` row (identity and lifecycle) and an
//! `entity_search` row (the indexed query axes), both keyed on the same
//! `entity_key`; its attributes are `current_attributes` rows keyed on it too,
//! which a fragment reaches by a probe correlated on that key. Every fragment
//! this crate renders reads at least one of them, and which relation a column
//! lives in is decided here once: a predicate asks the row references for "the
//! type ids column" or "the entity key" and never spells a relation of its own.
//!
//! The binding is carried as a value rather than assumed, because the enclosing
//! statement chooses how each relation is known: the entity path selects the
//! two tables unaliased, the temporal path joins them under aliases, a
//! traversal hop under others. A fragment that named a column unqualified would
//! rebind itself to whichever relation a later join happened to make visible,
//! so the outer row is always stated.

use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, IntoIden, SimpleExpr, TableRef};

/// The names the enclosing statement knows one entity's relations by.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRowRefs {
    /// The context branch schema every relation the statement names is
    /// qualified with, or `None` for the live branch's default schema. Column
    /// references stay bare `"table"."column"` and resolve against the range
    /// table entry the schema-qualified `FROM`/`JOIN` introduced.
    schema: Option<DynIden>,
    /// The `entities` relation, as the statement names it.
    identity: DynIden,
    /// The relation the statement reads an Entity's `entity_key` and `id` from:
    /// `entities` itself, or the type-membership walk that drives the
    /// statement and carries both, so the walk need not join `entities` for
    /// the two columns every statement reads.
    keys: DynIden,
    /// The `entity_search` relation, as the statement names it.
    ///
    /// A statement that filters on no search axis need not join it; the
    /// fragments that read it are the ones a caller only renders when it did.
    search: DynIden,
}

impl EntityRowRefs {
    /// The two relations used unaliased, under their own table names, in the
    /// given branch schema.
    #[must_use]
    pub fn unaliased(schema: Option<DynIden>) -> EntityRowRefs {
        EntityRowRefs {
            schema,
            identity: Entities::Table.into_iden(),
            keys: Entities::Table.into_iden(),
            search: EntitySearch::Table.into_iden(),
        }
    }

    /// The two relations as the statement aliases them, in the given branch
    /// schema.
    pub fn aliased(schema: Option<DynIden>, identity: impl IntoIden, search: impl IntoIden) -> EntityRowRefs {
        let identity = identity.into_iden();
        EntityRowRefs {
            schema,
            keys: identity.clone(),
            identity,
            search: search.into_iden(),
        }
    }

    /// The same relations, with `entity_key` and `id` read from `keys`: the
    /// relation that drives the statement and carries both.
    #[must_use]
    pub fn keyed_by(self, keys: impl IntoIden) -> EntityRowRefs {
        EntityRowRefs {
            keys: keys.into_iden(),
            ..self
        }
    }

    /// The branch schema this statement's relations are qualified with, for a
    /// fragment that reaches a further relation of its own.
    #[must_use]
    pub const fn schema(&self) -> Option<&DynIden> {
        self.schema.as_ref()
    }

    /// Qualify `table` with this statement's branch schema, for a `FROM` or
    /// `JOIN` a fragment introduces.
    #[must_use]
    pub fn table(&self, table: impl IntoIden) -> TableRef {
        qualified(self.schema.as_ref(), table)
    }

    /// A column of the `entities` row, qualified by the name the statement
    /// knows it under.
    ///
    /// The qualification is what makes a correlation a correlation. A subquery
    /// over `current_attributes` has an `entity_key` column of its own, and SQL
    /// resolves an unqualified name in the innermost scope that offers one, so
    /// `"ca"."entity_key" = "entity_key"` is not a reference to the outer row
    /// at all: it compares the subquery's column with itself and silently
    /// admits every instance in the tenant.
    ///
    /// `entity_key` and `id` are read from the relation that drives the
    /// statement, every other column from `entities` itself.
    #[must_use]
    pub fn identity_column(&self, column: Entities) -> SimpleExpr {
        let relation = match column {
            Entities::EntityKey | Entities::Id => &self.keys,
            Entities::Table
            | Entities::WriteCount
            | Entities::CreatedAt
            | Entities::ExpiresAt
            | Entities::MinAttributeExpiresAt
            | Entities::ModifiedAt
            | Entities::EvolutionCreatedAt
            | Entities::EvolutionModifiedAt
            | Entities::EvolutionTypeNameKeys => &self.identity,
        };
        Expr::col((relation.clone(), column.into_iden()))
    }

    /// The row's `entity_key`, the surrogate a correlated subquery binds against.
    #[must_use]
    pub fn entity_key_column(&self) -> SimpleExpr {
        self.identity_column(Entities::EntityKey)
    }

    /// A column of the `entity_search` row, qualified.
    #[must_use]
    pub fn search_column(&self, column: EntitySearch) -> SimpleExpr {
        Expr::col((self.search.clone(), column.into_iden()))
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_row_refs::EntityRowRefs;
    use andromeda_postgresql_schema::current::{Entities, EntitySearch};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn unaliased_references_qualify_by_the_table_names() {
        let rows = EntityRowRefs::unaliased(None);
        assert!(render(rows.entity_key_column()).contains(r#""entities"."entity_key""#));
        assert!(render(rows.identity_column(Entities::Id)).contains(r#""entities"."id""#));
        assert!(render(rows.search_column(EntitySearch::EntityTypeNameKeys)).contains(r#""entity_search"."entity_type_name_keys""#));
    }

    #[test]
    fn aliased_references_qualify_by_the_aliases() {
        let rows = EntityRowRefs::aliased(None, Alias::new("ce"), Alias::new("cs"));
        assert!(render(rows.entity_key_column()).contains(r#""ce"."entity_key""#));
        assert!(render(rows.search_column(EntitySearch::Location)).contains(r#""cs"."location""#));
    }

    /// A statement driven by the type-membership walk reads `entity_key` and
    /// `id` from the walk, while a lifecycle column still comes from
    /// `entities`.
    #[test]
    fn a_statement_keyed_by_its_driver_reads_the_key_and_the_identifier_from_it() {
        let rows = EntityRowRefs::unaliased(None).keyed_by(Alias::new("selected"));
        assert_eq!(render(rows.entity_key_column()), r#"SELECT "selected"."entity_key""#);
        assert_eq!(render(rows.identity_column(Entities::Id)), r#"SELECT "selected"."id""#);
        assert_eq!(render(rows.identity_column(Entities::CreatedAt)), r#"SELECT "entities"."created_at""#);
    }

    /// A Snapshot branch schema qualifies the tables a fragment reaches through
    /// `table`, while column references stay bare and resolve against them.
    #[test]
    fn a_branch_schema_qualifies_the_tables_a_fragment_reaches() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snap_5").into_iden()));
        let from = Query::select()
            .column(Entities::Id)
            .from(rows.table(Entities::Table))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(from.contains(r#"FROM "snap_5"."entities""#), "{from}");
    }

    #[test]
    fn no_reference_is_left_unqualified() {
        for rows in [EntityRowRefs::unaliased(None), EntityRowRefs::aliased(None, Alias::new("e"), Alias::new("s"))] {
            let sql = render(rows.entity_key_column());
            assert!(!sql.contains(r#"SELECT "entity_key""#), "{sql}");
        }
    }
}
