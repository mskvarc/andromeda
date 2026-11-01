//! The grains an update moves or removes, and the Entities whose staged rows
//! are every row they keep, staged beside the rows it writes whole.
//!
//! A grain travels as its Entity's identifier, its attribute name key, its
//! `datasetId` and the ordinal it moves to, null for a grain removed. The
//! grains are expanded with `UNNEST` under the alias `g`, in the common table
//! expression `grains`; the Entities whose rows are overwritten travel as one
//! identifier array.

use crate::current_attribute::row_edit::{EditReach, InstanceGrain, RowEdit};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_attribute_instance::{flatten::FlattenError, name_keys::NameKeys};
use andromeda_postgresql_query::dataset_key::DatasetKey;
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::{
    pg_array::{nullable_array, text_array},
    pg_type::PostgresType,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{CommonTableExpression, Expr, Iden, Query, SimpleExpr};

/// The grain staging alias, `g`.
///
/// Spelled nowhere else in a statement, so the single `AS "g"` its table
/// source emits is the one [`with_grains_alias`] names the columns of.
#[derive(Iden, Clone, Copy)]
#[iden = "g"]
struct Staged;

/// The common table expression, `grains`, an update reads the grains it moves
/// or removes from.
#[derive(Iden, Clone, Copy)]
#[iden = "grains"]
pub(crate) struct Grains;

/// The grain staging columns, `AS g(id, attribute_name_key, dataset_key, ordinal)`.
fn staged_columns() -> [UnnestColumn; 4] {
    [
        UnnestColumn::new(Entities::Id, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributes::AttributeNameKey, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributes::DatasetKey, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributes::Ordinal, PostgresType::IntegerArray),
    ]
}

/// The arrays an update's grains stage, as the expressions standing for them
/// in the statement.
pub(crate) struct GrainArrays {
    /// One array per grain staging column, in [`staged_columns`] order.
    pub(crate) grains: [SimpleExpr; 4],
    /// The identifiers of the Entities whose rows are overwritten.
    pub(crate) overwritten: SimpleExpr,
}

/// The `grains` CTE over the four grain `arrays`:
/// `grains AS (SELECT g.id, g.attribute_name_key, g.dataset_key, g.ordinal FROM UNNEST(…) AS g(…))`.
///
/// The statement it joins must be passed through [`with_grains_alias`] to name
/// the source's columns.
pub(crate) fn grains_cte(arrays: [SimpleExpr; 4]) -> CommonTableExpression {
    let columns = staged_columns();
    let source = unnest_call(columns.iter().zip(arrays));
    let select = Query::select()
        .expr_as(Expr::col((Staged, Entities::Id)), Entities::Id)
        .expr_as(Expr::col((Staged, CurrentAttributes::AttributeNameKey)), CurrentAttributes::AttributeNameKey)
        .expr_as(Expr::col((Staged, CurrentAttributes::DatasetKey)), CurrentAttributes::DatasetKey)
        .expr_as(Expr::col((Staged, CurrentAttributes::Ordinal)), CurrentAttributes::Ordinal)
        .from_function(source, Staged)
        .take();
    CommonTableExpression::new().table_name(Grains).query(select).to_owned()
}

/// `statement`, the grain staging's source named `g(id, …)` after its columns.
pub(crate) fn with_grains_alias<S>(statement: UnnestAliased<S>) -> UnnestAliased<S> {
    statement.with_columns(Staged, &staged_columns())
}

/// The grains of many Entities' edits, one vector per staged column, and the
/// Entities whose staged rows are every row they keep.
#[derive(Debug, Default)]
pub struct StagedGrains<'a> {
    /// The identifier of the Entity each grain belongs to.
    entity_ids: Vec<&'a str>,
    /// `attribute_name_key`.
    attribute_name_keys: Vec<i32>,
    /// `dataset_key`.
    dataset_keys: Vec<&'a str>,
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
            self.dataset_keys.push(DatasetKey::of(dataset_id).as_str());
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

    /// The grain columns and the overwritten Entities as the expressions
    /// standing for them in the statement.
    pub(crate) fn into_arrays(self) -> GrainArrays {
        GrainArrays {
            grains: [
                text_array(&self.entity_ids),
                Expr::val(self.attribute_name_keys),
                text_array(&self.dataset_keys),
                nullable_array(self.ordinals),
            ],
            overwritten: text_array(&self.overwritten),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        current_attribute::{
            row_edit::{EditReach, InstanceGrain, MovedInstance, RowEdit},
            staged_grains::{GrainArrays, StagedGrains, grains_cte, with_grains_alias},
        },
        statement_text::text_of,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
    use andromeda_postgresql_seaquery::unnest::UnnestAliased;
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
        pretty_assertions::assert_eq!(grains.dataset_keys, vec!["", "urn:ds:1"]);
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
    fn the_grain_staging_names_one_column_per_array() {
        let id = entity();
        let mut grains = StagedGrains::default();
        grains
            .push(
                &id,
                &RowEdit {
                    reach: EditReach::EveryRow,
                    written: Vec::new(),
                    moved: Vec::new(),
                    removed: Vec::new(),
                },
                &NameKeys::default(),
            )
            .expect("no grain to resolve");
        let GrainArrays { grains, .. } = grains.into_arrays();
        let mut with_clause = WithClause::new();
        with_clause.cte(grains_cte(grains));
        let sql = text_of(&with_grains_alias(UnnestAliased::new(with_clause.query(Query::select().take()))));
        assert!(
            sql.contains("UNNEST($1::text[], $2::integer[], $3::text[], $4::integer[]) AS g(id, attribute_name_key, dataset_key, ordinal)"),
            "{sql}"
        );
    }

    /// The overwritten Entities travel beside the grains as one text array.
    #[test]
    fn the_overwritten_entities_travel_as_one_text_array() {
        let edit = RowEdit {
            reach: EditReach::EveryRow,
            written: Vec::new(),
            moved: Vec::new(),
            removed: Vec::new(),
        };
        let id = entity();
        let mut grains = StagedGrains::default();
        grains.push(&id, &edit, &NameKeys::default()).expect("no grain to resolve");
        let (sql, values) = Query::select().expr(grains.into_arrays().overwritten).build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, "SELECT $1");
        pretty_assertions::assert_eq!(values.0.len(), 1);
    }
}
