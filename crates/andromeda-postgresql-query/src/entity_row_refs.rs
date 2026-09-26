//! How a statement reaches the three relations that hold one entity's
//! current state.
//!
//! An entity is an `entities` row (identity and lifecycle), an
//! `entity_search` row (the indexed query axes) and an `entity_documents` row
//! (the payload), all keyed on the same `entity_key`. Every fragment this
//! crate renders reads at least one of them, and which relation a column lives
//! in is decided here once: a predicate asks the row references for "the type
//! ids column" or "the payload" and never spells a relation of its own.
//!
//! The binding is carried as a value rather than assumed, because the enclosing
//! statement chooses how each relation is known: the entity path selects the
//! three tables unaliased, the temporal path joins them under aliases, a
//! traversal hop under others. A fragment that named a column unqualified would
//! rebind itself to whichever relation a later join happened to make visible,
//! so the outer row is always stated.

use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, ExprTrait, IntoIden, Query, SimpleExpr, TableRef};

/// How the enclosing statement reaches the payload of the entity.
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentsRef {
    /// `entity_documents` is joined into the statement under this name, so the
    /// payload is a plain column reference.
    Joined(DynIden),
    /// `entity_documents` is not joined; the payload is read by a scalar
    /// subquery keyed on the identity row's `entity_key`, so a statement that
    /// rarely needs the payload does not pay for the join on every row.
    ByKeyLookup,
}

impl DocumentsRef {
    /// The payload of an `entity_documents` row joined under `documents`.
    pub fn joined(documents: impl IntoIden) -> DocumentsRef {
        DocumentsRef::Joined(documents.into_iden())
    }
}

/// The names the enclosing statement knows one entity's three relations by.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRowRefs {
    /// The context branch schema every relation the statement names is
    /// qualified with, or `None` for the live branch's default schema. Column
    /// references stay bare `"table"."column"` and resolve against the range
    /// table entry the schema-qualified `FROM`/`JOIN` introduced.
    schema: Option<DynIden>,
    /// The `entities` relation, as the statement names it.
    identity: DynIden,
    /// The `entity_search` relation, as the statement names it.
    ///
    /// A statement that filters on no search axis need not join it; the
    /// fragments that read it are the ones a caller only renders when it did.
    search: DynIden,
    /// How the payload is reached.
    documents: DocumentsRef,
}

impl EntityRowRefs {
    /// The three relations used unaliased, under their own table names, with
    /// `entity_documents` joined, in the given branch schema.
    #[must_use]
    pub fn unaliased(schema: Option<DynIden>) -> EntityRowRefs {
        EntityRowRefs::unaliased_with(schema, DocumentsRef::Joined(EntityDocuments::Table.into_iden()))
    }

    /// The identity and search relations used unaliased, under their own table
    /// names, in the given branch schema, with the payload reached as
    /// `documents` says.
    ///
    /// A statement that projects the payload but neither filters nor sorts on
    /// it reads it [`DocumentsRef::ByKeyLookup`]: the planner then defers the
    /// lookup past the sort and the limit, so a page of a hundred entities
    /// fetches a hundred documents instead of joining every candidate's.
    #[must_use]
    pub fn unaliased_with(schema: Option<DynIden>, documents: DocumentsRef) -> EntityRowRefs {
        EntityRowRefs {
            schema,
            identity: Entities::Table.into_iden(),
            search: EntitySearch::Table.into_iden(),
            documents,
        }
    }

    /// The three relations as the statement aliases them, in the given branch
    /// schema.
    pub fn aliased(schema: Option<DynIden>, identity: impl IntoIden, search: impl IntoIden, documents: DocumentsRef) -> EntityRowRefs {
        EntityRowRefs {
            schema,
            identity: identity.into_iden(),
            search: search.into_iden(),
            documents,
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
    /// over `current_attribute_values` has an `entity_key` column of its own,
    /// and SQL resolves an unqualified name in the innermost scope that offers
    /// one, so `"cav"."entity_key" = "entity_key"` is not a reference to the
    /// outer row at all — it compares the subquery's column with itself and
    /// silently admits every instance in the tenant.
    #[must_use]
    pub fn identity_column(&self, column: Entities) -> SimpleExpr {
        Expr::col((self.identity.clone(), column.into_iden()))
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

    /// The `entity_search` relation's name, for a statement that joins it.
    #[must_use]
    pub fn search_relation(&self) -> DynIden {
        self.search.clone()
    }

    /// A column of the `entity_documents` row, reached as the joined row's
    /// column or, when the row is not joined, by a scalar subquery keyed on the
    /// identity row -- the same two shapes the payload is reached in.
    #[must_use]
    pub fn documents_column(&self, column: EntityDocuments) -> SimpleExpr {
        match &self.documents {
            DocumentsRef::Joined(documents) => Expr::col((documents.clone(), column.into_iden())),
            DocumentsRef::ByKeyLookup => Query::select()
                .column(column)
                .from(qualified(self.schema.as_ref(), EntityDocuments::Table))
                .and_where(Expr::col(EntityDocuments::EntityKey).eq(self.entity_key_column()))
                .take()
                .into(),
        }
    }

    /// The payload, as a column of the joined `entity_documents` row or as a
    /// scalar subquery keyed on the identity row.
    #[must_use]
    pub fn payload_column(&self) -> SimpleExpr {
        self.documents_column(EntityDocuments::Payload)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_row_refs::{DocumentsRef, EntityRowRefs};
    use andromeda_postgresql_schema::current::{Entities, EntitySearch};
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
        assert!(render(rows.payload_column()).contains(r#""entity_documents"."payload""#));
    }

    #[test]
    fn aliased_references_qualify_by_the_aliases() {
        let rows = EntityRowRefs::aliased(None, Alias::new("ce"), Alias::new("cs"), DocumentsRef::joined(Alias::new("cd")));
        assert!(render(rows.entity_key_column()).contains(r#""ce"."entity_key""#));
        assert!(render(rows.search_column(EntitySearch::Location)).contains(r#""cs"."location""#));
        assert!(render(rows.payload_column()).contains(r#""cd"."payload""#));
    }

    #[test]
    fn a_payload_read_by_key_lookup_is_a_scalar_subquery_correlated_on_the_identity_row() {
        let rows = EntityRowRefs::aliased(None, Alias::new("ce"), Alias::new("cs"), DocumentsRef::ByKeyLookup);
        let sql = render(rows.payload_column());
        assert!(
            sql.contains(r#"(SELECT "payload" FROM "entity_documents" WHERE "entity_key" = "ce"."entity_key")"#),
            "{sql}"
        );
    }

    /// A Snapshot branch schema qualifies the tables a fragment reaches -- the
    /// key-lookup subquery's `FROM` and any table introduced through `table` --
    /// while column references stay bare and resolve against them.
    #[test]
    fn a_branch_schema_qualifies_the_tables_a_fragment_reaches() {
        let schema = Alias::new("snap_5").into_iden();
        let rows = EntityRowRefs::unaliased_with(Some(schema), DocumentsRef::ByKeyLookup);
        assert!(
            render(rows.payload_column()).contains(r#"FROM "snap_5"."entity_documents""#),
            "{}",
            render(rows.payload_column())
        );
        let from = Query::select()
            .column(Entities::Id)
            .from(rows.table(Entities::Table))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(from.contains(r#"FROM "snap_5"."entities""#), "{from}");
    }

    #[test]
    fn no_reference_is_left_unqualified() {
        for rows in [
            EntityRowRefs::unaliased(None),
            EntityRowRefs::aliased(None, Alias::new("e"), Alias::new("s"), DocumentsRef::ByKeyLookup),
        ] {
            let sql = render(rows.entity_key_column());
            assert!(!sql.contains(r#"SELECT "entity_key""#), "{sql}");
        }
    }
}
