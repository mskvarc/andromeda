//! The grains an update moves or removes, and the Entities whose staged rows
//! are every row they keep, staged beside the rows it writes whole.
//!
//! A grain travels as its Entity's identifier, its attribute name key, its
//! `datasetId` and the ordinal it moves to, null for a grain removed. The
//! grains are expanded with `UNNEST` under the alias `g`, in the common table
//! expression `grains`; the Entities whose rows are overwritten travel as one
//! identifier array.

use crate::current_attribute::{
    flatten::FlattenError,
    name_keys::NameKeys,
    row_edit::{EditReach, InstanceGrain, RowEdit},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{CommonTableExpression, Expr, Iden, Query};
use sqlx::{Postgres, postgres::PgArguments, query::QueryAs};

/// The grain staging alias, `g`.
///
/// Spelled nowhere else in a statement, so the single `AS "g"` its table
/// source emits is the one [`apply_grains_alias`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "g"]
struct Staged;

/// The common table expression, `grains`, an update reads the grains it moves
/// or removes from.
#[derive(Iden, Clone, Copy)]
#[iden = "grains"]
pub(crate) struct Grains;

/// The number of binds the grains take: the four grain columns, then the
/// identifiers of the Entities whose rows are overwritten.
pub(crate) const GRAIN_BINDS: usize = 5;

/// The grain staging columns, `AS g(id, attribute_name_key, dataset_id, ordinal)`.
fn staged_columns() -> [UnnestColumn; 4] {
    [
        UnnestColumn::new(Entities::Id, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributes::AttributeNameKey, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributes::DatasetId, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributes::Ordinal, PostgresType::IntegerArray),
    ]
}

/// The `grains` CTE over the binds from placeholder `first` on:
/// `grains AS (SELECT g.id, g.attribute_name_key, g.dataset_id, g.ordinal FROM UNNEST($first::text[], …) AS g(…))`.
pub(crate) fn grains_cte(first: usize) -> CommonTableExpression {
    let columns = staged_columns();
    let source = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(first + index))));
    let select = Query::select()
        .expr_as(Expr::col((Staged, Entities::Id)), Entities::Id)
        .expr_as(Expr::col((Staged, CurrentAttributes::AttributeNameKey)), CurrentAttributes::AttributeNameKey)
        .expr_as(Expr::col((Staged, CurrentAttributes::DatasetId)), CurrentAttributes::DatasetId)
        .expr_as(Expr::col((Staged, CurrentAttributes::Ordinal)), CurrentAttributes::Ordinal)
        .from_function(source, Staged)
        .take();
    CommonTableExpression::new().table_name(Grains).query(select).to_owned()
}

/// Rewrite the grain staging's `AS "g"` into `AS g(id, …)`.
pub(crate) fn apply_grains_alias(sql: String) -> String {
    apply_column_alias(sql, &Staged, &staged_columns())
}

/// The grains of many Entities' edits, one vector per staged column, and the
/// Entities whose staged rows are every row they keep.
#[derive(Debug, Default)]
pub struct StagedGrains<'a> {
    /// The identifier of the Entity each grain belongs to.
    entity_ids: Vec<&'a str>,
    /// `attribute_name_key`.
    attribute_name_keys: Vec<i32>,
    /// `dataset_id`.
    dataset_ids: Vec<Option<&'a str>>,
    /// The ordinal a moved grain takes, null for a removed one.
    ordinals: Vec<Option<i32>>,
    /// The Entities whose staged rows are every row they keep.
    overwritten: Vec<&'a str>,
}

impl<'a> StagedGrains<'a> {
    /// Stage the grains `edit` moves or removes of the Entity `entity`.
    ///
    /// # Errors
    ///
    /// Returns [`FlattenError::UnresolvedAttribute`] when a grain's attribute
    /// name has no key in `keys`.
    pub fn push(&mut self, entity: &'a Identifier, edit: &RowEdit<'a>, keys: &NameKeys) -> Result<(), FlattenError> {
        match edit.reach {
            EditReach::EveryRow => self.overwritten.push(entity.as_str()),
            EditReach::NamedRows => {}
        }
        let moved = edit.moved.iter().map(|moved| (moved.grain, Some(moved.ordinal)));
        let removed = edit.removed.iter().map(|grain| (*grain, None));
        for (InstanceGrain { name, dataset_id }, ordinal) in moved.chain(removed) {
            let key = keys.attribute(name).ok_or_else(|| FlattenError::UnresolvedAttribute(name.clone()))?;
            self.entity_ids.push(entity.as_str());
            self.attribute_name_keys.push(key.get());
            self.dataset_ids.push(dataset_id.map(Identifier::as_str));
            self.ordinals.push(ordinal);
        }
        Ok(())
    }

    /// The number of grains staged.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entity_ids.len()
    }

    /// Whether no grain is staged.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entity_ids.is_empty()
    }

    /// Bind the grain columns, then the overwritten Entities, to `query`, in
    /// the order [`grains_cte`] and the statement read them.
    pub(crate) fn bind_to<'q, O>(self, query: QueryAs<'q, Postgres, O, PgArguments>) -> QueryAs<'q, Postgres, O, PgArguments>
    where
        'a: 'q,
    {
        query
            .bind(self.entity_ids)
            .bind(self.attribute_name_keys)
            .bind(self.dataset_ids)
            .bind(self.ordinals)
            .bind(self.overwritten)
    }
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::{
        name_keys::NameKeys,
        row_edit::{EditReach, InstanceGrain, MovedInstance, RowEdit},
        staged_grains::{StagedGrains, apply_grains_alias, grains_cte},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
    use sea_query::{PostgresQueryBuilder, Query, WithClause};

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    fn entity() -> Identifier {
        "urn:ngsi-ld:Building:1".parse().expect("a valid identifier")
    }

    /// A moved grain carries the ordinal it moves to and a removed one none;
    /// an edit that names its rows overwrites nothing.
    #[test]
    fn a_moved_grain_carries_its_ordinal_and_a_removed_one_none() {
        let (level, pressure) = (name("https://ex.org/level"), name("https://ex.org/pressure"));
        let first: Identifier = "urn:ds:1".parse().expect("a valid dataset identifier");
        let keys = NameKeys::new(
            ResolvedNames::from_pairs([(level.clone(), AttributeNameKey::from(3)), (pressure.clone(), AttributeNameKey::from(4))]),
            ResolvedNames::default(),
        );
        let edit = RowEdit {
            reach: EditReach::NamedRows,
            written: Vec::new(),
            moved: vec![MovedInstance {
                grain: InstanceGrain {
                    name: &pressure,
                    dataset_id: None,
                },
                ordinal: 2,
            }],
            removed: vec![InstanceGrain {
                name: &level,
                dataset_id: Some(&first),
            }],
        };
        let id = entity();
        let mut grains = StagedGrains::default();
        grains.push(&id, &edit, &keys).expect("every grain resolves");
        pretty_assertions::assert_eq!(grains.attribute_name_keys, vec![4, 3]);
        pretty_assertions::assert_eq!(grains.dataset_ids, vec![None, Some("urn:ds:1")]);
        pretty_assertions::assert_eq!(grains.ordinals, vec![Some(2), None]);
        pretty_assertions::assert_eq!(grains.overwritten, Vec::<&str>::new());
    }

    /// An edit of every row stages no grain and names its Entity as
    /// overwritten.
    #[test]
    fn an_edit_of_every_row_names_its_entity_as_overwritten() {
        let edit = RowEdit {
            reach: EditReach::EveryRow,
            written: Vec::new(),
            moved: Vec::new(),
            removed: Vec::new(),
        };
        let id = entity();
        let mut grains = StagedGrains::default();
        grains.push(&id, &edit, &NameKeys::default()).expect("no grain to resolve");
        assert!(grains.is_empty());
        pretty_assertions::assert_eq!(grains.overwritten, vec!["urn:ngsi-ld:Building:1"]);
    }

    /// A grain whose name the dictionary did not resolve cannot be staged.
    #[test]
    fn a_grain_without_a_key_is_refused() {
        let level = name("https://ex.org/level");
        let edit = RowEdit {
            reach: EditReach::NamedRows,
            written: Vec::new(),
            moved: Vec::new(),
            removed: vec![InstanceGrain {
                name: &level,
                dataset_id: None,
            }],
        };
        let id = entity();
        assert!(StagedGrains::default().push(&id, &edit, &NameKeys::default()).is_err());
    }

    #[test]
    fn the_grain_staging_names_one_column_per_bind() {
        let mut with_clause = WithClause::new();
        with_clause.cte(grains_cte(4));
        let sql = apply_grains_alias(with_clause.query(Query::select().take()).to_string(PostgresQueryBuilder));
        assert!(
            sql.contains("UNNEST($4::text[], $5::integer[], $6::text[], $7::integer[]) AS g(id, attribute_name_key, dataset_id, ordinal)"),
            "{sql}"
        );
    }
}
