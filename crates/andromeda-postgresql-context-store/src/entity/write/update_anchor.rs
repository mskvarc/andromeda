//! The anchor of the batch update: the identity row write that counts the
//! write, stamps the clause 4.8 `modifiedAt`, holds the compare-and-set, and
//! refuses a write stamped before the latest instant the Entity holds.

use crate::entity::write::{
    latest_instant::{latest_instant, latest_where_refused, predates, unless_refused},
    staging::projected,
    statement_ctes::Proj,
    version_guard::{expectation, next_write_count},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::entity_sides::holds_current_state;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::returning_image::{New, Old};
use chrono::{DateTime, Utc};
use sea_query::{Cond, Expr, ExprTrait, Query, UpdateStatement};

/// The update's anchor, `stamped`: each identity row holding a current state
/// whose expectation still holds, stamped and given its new expiries unless
/// it holds an instant later than `accepted_at`, with the key and identifier
/// of every row it met and the latest instant of each it refused. A row whose
/// current state a core deletion removed
/// keeps only its Temporal Evolution, which no core update reaches.
///
/// It finds each row through the unique identifier index, so its cost grows
/// with the batch whatever row estimate the plan was made for. A refused row
/// keeps its write count, which is how the returned images tell it apart.
pub(super) fn update_anchor(target: &ContextTarget, accepted_at: DateTime<Utc>) -> UpdateStatement {
    let refused = predates(latest_instant(Entities::Table), accepted_at);
    let kept = |column: Entities| Expr::col((Entities::Table, column));
    let unchanged_count = Expr::col((New, Entities::WriteCount)).equals((Old, Entities::WriteCount));
    Query::update()
        .table(table_of(target, Entities::Table))
        .value(
            Entities::WriteCount,
            unless_refused(&refused, kept(Entities::WriteCount), next_write_count(Entities::Table)),
        )
        .value(
            Entities::ModifiedAt,
            unless_refused(&refused, kept(Entities::ModifiedAt), Expr::val(accepted_at)),
        )
        .value(
            Entities::ExpiresAt,
            unless_refused(&refused, kept(Entities::ExpiresAt), projected(Entities::ExpiresAt)),
        )
        .value(
            Entities::MinAttributeExpiresAt,
            unless_refused(&refused, kept(Entities::MinAttributeExpiresAt), projected(Entities::MinAttributeExpiresAt)),
        )
        .from(Proj)
        .cond_where(
            Cond::all()
                .add(Expr::col((Entities::Table, Entities::Id)).equals((Proj, Entities::Id)))
                .add(holds_current_state(Entities::Table))
                .add(expectation(Proj, Entities::Table)),
        )
        .returning(Query::returning().exprs([
            Expr::col((Entities::Table, Entities::EntityKey)),
            Expr::col((Entities::Table, Entities::Id)),
            latest_where_refused(unchanged_count, latest_instant(Old)),
        ]))
        .take()
}
