//! The clause 5.12 `datasetId` rule.
//!
//! A request carrying `datasetId` matches a registration that also declares a
//! `datasetId` list only when the two share a value; a registration declaring
//! none is matched regardless, because only one side specifies one.

use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{pg_array::text_array, pg_operator::PgBinaryOperator};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait};

/// `dataset_id IS NULL OR dataset_id && ARRAY[...]`, in the clause 4.5.5 wire
/// form of each selector (`@none` names the default instance).
pub(crate) fn dataset_rule(selectors: &[DatasetSelector]) -> Condition {
    let wire: Vec<String> = selectors.iter().map(ToString::to_string).collect();
    let wire: Vec<&str> = wire.iter().map(String::as_str).collect();
    Condition::any()
        .add(SeaExpr::col(Csr::DatasetId).is_null())
        .add(SeaExpr::expr(SeaExpr::col(Csr::DatasetId)).array_overlaps(text_array(&wire)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn a_registration_without_dataset_ids_matches_and_one_with_them_must_share_a_value() {
        let selectors = [
            DatasetSelector::Default,
            DatasetSelector::Id("urn:ngsi-ld:dataset:1".parse().expect("valid URI")),
        ];
        let sql = Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .cond_where(dataset_rule(&selectors))
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("\"dataset_id\" IS NULL OR (\"dataset_id\" && ARRAY ['@none','urn:ngsi-ld:dataset:1'])"),
            "{sql}"
        );
    }
}
