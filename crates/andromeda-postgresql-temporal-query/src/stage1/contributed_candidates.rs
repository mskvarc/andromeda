//! The candidates every Entity a Context Source contributed to a split query
//! adds to a step whose index answers a filter clause 5.7.4.4 applies only
//! after aggregation.
//!
//! Clause 5.7.4.4 removes the `q`, the geoquery and the Scope query from the
//! local selection of a split query and applies them once the parts from the
//! Context Sources are aggregated, so the local rows of a contributed Entity
//! decide nothing about those filters, and no index of them can exclude it.

use andromeda_ngsi_ld_query::temporal::contributed_entities::ContributedEntities;
use andromeda_postgresql_query::history_source::{HistoryBranch, PendingEntries};
use andromeda_postgresql_schema::temporal::TemporalEntities;
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Expr, ExprTrait, Query, SelectStatement};

/// The Evolutions of `branch` of every Entity `contributed` names: their
/// surrogate keys over recorded rows alone, their identifiers where the
/// branch reads its queue; `None` when the query was not split.
pub(super) fn contributed_candidates(branch: &HistoryBranch, contributed: &ContributedEntities) -> Option<SelectStatement> {
    if contributed.is_empty() {
        return None;
    }
    let column = match branch.pending() {
        PendingEntries::Absent => TemporalEntities::TemporalEntityKey,
        PendingEntries::Unioned => TemporalEntities::Id,
    };
    let ids: Vec<&str> = contributed.as_slice().iter().map(AsRef::as_ref).collect();
    Some(
        Query::select()
            .column((TemporalEntities::Table, column))
            .from(qualified(branch.schema(), TemporalEntities::Table))
            .and_where(Expr::col((TemporalEntities::Table, TemporalEntities::Id)).is_in(ids))
            .take(),
    )
}

#[cfg(test)]
mod tests {
    use crate::stage1::{contributed_candidates::contributed_candidates, query_fixture::ids};
    use andromeda_ngsi_ld_query::temporal::contributed_entities::{ContributedEntities, ContributedRendering};
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    /// Two Entities a Context Source contributed.
    fn two() -> ContributedEntities {
        ContributedEntities::new(ids(&["urn:ngsi-ld:Sensor:000009", "urn:ngsi-ld:Sensor:000011"]), ContributedRendering::Local)
    }

    #[test]
    fn a_query_never_split_adds_no_candidate() {
        assert!(contributed_candidates(&HistoryBranch::live_recorded(), &ContributedEntities::default()).is_none());
    }

    #[test]
    fn recorded_rows_name_a_contributed_entity_by_its_evolution_key() {
        let sql = contributed_candidates(&HistoryBranch::live_recorded(), &two())
            .expect("contributed Entities")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "temporal_entities"."temporal_entity_key" FROM "temporal_entities""#,
                r#" WHERE "temporal_entities"."id" IN ('urn:ngsi-ld:Sensor:000009', 'urn:ngsi-ld:Sensor:000011')"#
            )
        );
    }

    #[test]
    fn a_queued_branch_names_a_contributed_entity_by_its_identifier() {
        let sql = contributed_candidates(&HistoryBranch::live(), &two())
            .expect("contributed Entities")
            .to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "temporal_entities"."id" FROM "temporal_entities" WHERE"#), "{sql}");
    }

    #[test]
    fn a_snapshot_reads_its_own_catalogue() {
        let sql = contributed_candidates(&HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), &two())
            .expect("contributed Entities")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_7"."temporal_entities" WHERE"#), "{sql}");
    }
}
