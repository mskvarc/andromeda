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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::dataset_predicate::dataset_condition;
    use andromeda_ngsi_ld_query::dataset::DatasetFilter;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(filter: &DatasetFilter) -> String {
        let column: SimpleExpr = Expr::col((Alias::new("s"), Alias::new("dataset_key")));
        Query::select()
            .expr(Expr::val(1))
            .cond_where(dataset_condition(&column, filter))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.5.5: `@none` selects the default instance, whose grain key is
    /// the empty string.
    #[test]
    fn each_selection_renders_its_own_test() {
        let named = vec!["urn:ngsi-ld:Dataset:one".parse().expect("a valid URI")];
        assert!(!render(&DatasetFilter::Unfiltered).contains("dataset_key"));
        assert!(render(&DatasetFilter::DefaultOnly).contains(r#""s"."dataset_key" = ('')"#));
        let only = render(&DatasetFilter::NamedOnly(named.clone()));
        assert!(only.contains(r#""s"."dataset_key" = 'urn:ngsi-ld:Dataset:one'"#), "{only}");
        let both = render(&DatasetFilter::DefaultOrNamed(named));
        assert!(both.contains(r#""s"."dataset_key" = ANY(ARRAY ['urn:ngsi-ld:Dataset:one',''])"#), "{both}");
        assert!(!both.contains(" OR "), "{both}");
    }
}
