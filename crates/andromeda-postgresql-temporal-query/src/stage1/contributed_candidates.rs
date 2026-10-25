//! The candidates every Entity a Context Source contributed to a split query
//! adds to a step whose index answers a filter clause 5.7.4.4 applies only
//! after aggregation.
//!
//! Clause 5.7.4.4 removes the `q`, the geoquery and the Scope query from the
//! local selection of a split query and applies them once the parts from the
//! Context Sources are aggregated, so the local rows of a contributed Entity
//! decide nothing about those filters, and no index of them can exclude it.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::contributed_entities::ContributedEntities;
use andromeda_postgresql_query::{
    entity_sides::holds_evolution,
    history_source::{HistoryBranch, PendingEntries},
};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::is_any_of};
use sea_query::{Expr, Query, SelectStatement};

/// The Evolutions of `branch` of every Entity `contributed` names: their
/// surrogate keys over recorded rows alone, their identifiers where the
/// branch reads its queue; `None` when the query was not split.
pub(super) fn contributed_candidates(branch: &HistoryBranch, contributed: &ContributedEntities) -> Option<SelectStatement> {
    if contributed.is_empty() {
        return None;
    }
    let column = match branch.pending() {
        PendingEntries::Absent => Entities::EntityKey,
        PendingEntries::Unioned => Entities::Id,
    };
    Some(
        Query::select()
            .column((Entities::Table, column))
            .from(qualified(branch.schema(), Entities::Table))
            .and_where(is_any_of(
                Expr::col((Entities::Table, Entities::Id)),
                contributed.as_slice().iter().map(Identifier::to_string),
            ))
            .and_where(holds_evolution(Entities::Table))
            .take(),
    )
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
                r#"SELECT "entities"."entity_key" FROM "entities""#,
                r#" WHERE "entities"."id" = ANY(ARRAY ['urn:ngsi-ld:Sensor:000009','urn:ngsi-ld:Sensor:000011'])"#,
                r#" AND "entities"."evolution_created_at" IS NOT NULL"#
            )
        );
    }

    #[test]
    fn a_queued_branch_names_a_contributed_entity_by_its_identifier() {
        let sql = contributed_candidates(&HistoryBranch::live(), &two())
            .expect("contributed Entities")
            .to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "entities"."id" FROM "entities" WHERE"#), "{sql}");
    }

    #[test]
    fn a_snapshot_reads_its_own_catalogue() {
        let sql = contributed_candidates(&HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), &two())
            .expect("contributed Entities")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_7"."entities" WHERE"#), "{sql}");
    }
}
