//! A clause 4.5.5 `datasetId` selection as a predicate on one `dataset_id`
//! column.
//!
//! The default instance carries no `datasetId` and is stored as a NULL
//! `dataset_id`. The column is tested as the instance grain's unique index
//! keys it, the default instance as the empty string, so the default and the
//! named instances are one membership test and an index on the grain answers
//! it.

use andromeda_ngsi_ld_query::dataset::DatasetFilter;
use andromeda_postgresql_seaquery::{
    nullable_text_key::{is_null_text_key, nullable_text_key},
    pg_array::is_any_of,
};
use sea_query::{Condition, SimpleExpr};

/// Whether the instance whose `dataset_id` is `column` is one `filter` selects.
#[must_use]
pub fn dataset_condition(column: &SimpleExpr, filter: &DatasetFilter) -> Condition {
    // clone: sea-query owns every operand
    let dataset = || column.clone();
    match filter {
        DatasetFilter::Unfiltered => Condition::all(),
        DatasetFilter::DefaultOnly => Condition::all().add(is_null_text_key(dataset())),
        DatasetFilter::NamedOnly(named) => Condition::all().add(is_any_of(nullable_text_key(dataset()), named.iter().map(ToString::to_string))),
        DatasetFilter::DefaultOrNamed(named) => Condition::all().add(is_any_of(
            nullable_text_key(dataset()),
            named.iter().map(ToString::to_string).chain([String::new()]),
        )),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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

    /// Clause 4.5.5: `@none` selects the default instance, whose column is
    /// NULL and whose grain key is the empty string.
    #[test]
    fn each_selection_renders_its_own_test() {
        let named = vec!["urn:ngsi-ld:Dataset:one".parse().expect("a valid URI")];
        assert!(!render(&DatasetFilter::Unfiltered).contains("dataset_id"));
        assert!(render(&DatasetFilter::DefaultOnly).contains(r#"COALESCE("s"."dataset_id", '') = ('')"#));
        let only = render(&DatasetFilter::NamedOnly(named.clone()));
        assert!(
            only.contains(r#"COALESCE("s"."dataset_id", '') = ANY(ARRAY ['urn:ngsi-ld:Dataset:one'])"#),
            "{only}"
        );
        let both = render(&DatasetFilter::DefaultOrNamed(named));
        assert!(
            both.contains(r#"COALESCE("s"."dataset_id", '') = ANY(ARRAY ['urn:ngsi-ld:Dataset:one',''])"#),
            "{both}"
        );
        assert!(!both.contains(" OR "), "{both}");
    }
}
