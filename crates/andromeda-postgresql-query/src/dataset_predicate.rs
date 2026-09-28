//! A clause 4.5.5 `datasetId` selection as a predicate on one `dataset_id`
//! column.
//!
//! The default instance carries no `datasetId` and is stored as a NULL
//! `dataset_id`, so it is tested with `IS NULL` rather than by the membership
//! test that selects the named ones.

use andromeda_ngsi_ld_query::dataset::DatasetFilter;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Whether the instance whose `dataset_id` is `column` is one `filter` selects.
#[must_use]
pub fn dataset_condition(column: &SimpleExpr, filter: &DatasetFilter) -> Condition {
    // clone: sea-query owns every operand, and one column may be tested twice
    let dataset = || SeaExpr::expr(column.clone());
    match filter {
        DatasetFilter::Unfiltered => Condition::all(),
        DatasetFilter::DefaultOnly => Condition::all().add(dataset().is_null()),
        DatasetFilter::NamedOnly(named) => Condition::all().add(dataset().is_in(named.iter().map(ToString::to_string))),
        DatasetFilter::DefaultOrNamed(named) => Condition::all().add(
            Condition::any()
                .add(dataset().is_in(named.iter().map(ToString::to_string)))
                .add(dataset().is_null()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::dataset_predicate::dataset_condition;
    use andromeda_ngsi_ld_query::dataset::DatasetFilter;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(filter: &DatasetFilter) -> String {
        let column: SimpleExpr = Expr::col((Alias::new("s"), Alias::new("dataset_id")));
        Query::select()
            .expr(Expr::val(1))
            .cond_where(dataset_condition(&column, filter))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.5.5: `@none` selects the default instance, whose column is NULL.
    #[test]
    fn each_selection_renders_its_own_test() {
        let named = vec!["urn:ngsi-ld:Dataset:one".parse().expect("a valid URI")];
        assert!(!render(&DatasetFilter::Unfiltered).contains("dataset_id"));
        assert!(render(&DatasetFilter::DefaultOnly).contains(r#""s"."dataset_id" IS NULL"#));
        let only = render(&DatasetFilter::NamedOnly(named.clone()));
        assert!(only.contains(r#""s"."dataset_id" IN ('urn:ngsi-ld:Dataset:one')"#), "{only}");
        assert!(!only.contains("IS NULL"), "{only}");
        let both = render(&DatasetFilter::DefaultOrNamed(named));
        assert!(both.contains(r#"IN ('urn:ngsi-ld:Dataset:one') OR "s"."dataset_id" IS NULL"#), "{both}");
    }
}
