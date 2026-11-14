//! A clause 4.5.5 `datasetId` selection as a predicate on one grain key
//! column.
//!
//! The column is a [`DatasetKey`], so the default and the named instances are
//! one membership test with plain `=`, and an index on the grain answers it.

use crate::dataset_key::{DatasetKey, is_default_instance};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::dataset::DatasetFilter;
use andromeda_postgresql_seaquery::pg_array::is_any_of;
use sea_query::{Condition, SimpleExpr};

/// Whether the instance whose grain key is `column` is one `filter` selects.
#[must_use]
pub fn dataset_condition(column: &SimpleExpr, filter: &DatasetFilter) -> Condition {
    // clone: sea-query owns every operand
    let dataset = || column.clone();
    let keys = |named: &[Identifier]| {
        named
            .iter()
            .map(|dataset_id| DatasetKey::of(Some(dataset_id)).as_str().to_owned())
            .collect::<Vec<String>>()
    };
    match filter {
        DatasetFilter::Unfiltered => Condition::all(),
        DatasetFilter::DefaultOnly => Condition::all().add(is_default_instance(dataset())),
        DatasetFilter::NamedOnly(named) => Condition::all().add(is_any_of(dataset(), keys(named))),
        DatasetFilter::DefaultOrNamed(named) => {
            Condition::all().add(is_any_of(dataset(), keys(named).into_iter().chain([DatasetKey::of(None).as_str().to_owned()])))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dataset_predicate::dataset_condition;
    use andromeda_ngsi_ld_query::dataset::DatasetFilter;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr, Query, SelectStatement, SimpleExpr};

    fn render(filter: &DatasetFilter) -> SelectStatement {
        let column: SimpleExpr = Expr::col((Alias::new("s"), Alias::new("dataset_key")));
        Query::select().expr(Expr::val(1)).cond_where(dataset_condition(&column, filter)).to_owned()
    }

    /// Clause 4.5.5: `@none` selects the default instance, whose grain key is
    /// the empty string.
    #[test]
    fn each_selection_renders_its_own_test() {
        let named = vec!["urn:ngsi-ld:Dataset:one".parse().expect("a valid URI")];
        assert_statement_snapshot!("dataset-predicate-each-selection-renders-its-own-test-1", render(&DatasetFilter::Unfiltered));
        assert_statement_snapshot!("dataset-predicate-each-selection-renders-its-own-test-2", render(&DatasetFilter::DefaultOnly));
        assert_statement_snapshot!(
            "dataset-predicate-each-selection-renders-its-own-test-3",
            render(&DatasetFilter::NamedOnly(named.clone()))
        );
        assert_statement_snapshot!(
            "dataset-predicate-each-selection-renders-its-own-test-4",
            render(&DatasetFilter::DefaultOrNamed(named))
        );
    }
}
