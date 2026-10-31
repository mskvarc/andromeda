//! The `active_since` and `selected_since` values a subscription write stores.

use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::clock::now;
use andromeda_subscription::notifying_since::{NotifyingRestart, SinceRestart};
use sea_query::SimpleExpr;

/// Both instants of a created subscription: the creating transaction's
/// timestamp.
pub(crate) fn created_since() -> [(Subscriptions, SimpleExpr); 2] {
    [(Subscriptions::ActiveSince, now()), (Subscriptions::SelectedSince, now())]
}

/// The instants `restart` restarts at the updating transaction's timestamp; a
/// kept instant is not written.
pub(crate) fn restarted_since(restart: NotifyingRestart) -> Vec<(Subscriptions, SimpleExpr)> {
    [(Subscriptions::ActiveSince, restart.active), (Subscriptions::SelectedSince, restart.selected)]
        .into_iter()
        .filter_map(|(column, since)| match since {
            SinceRestart::Restarted => Some((column, now())),
            SinceRestart::Kept => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::notifying_columns::{created_since, restarted_since};
    use andromeda_postgresql_schema::subscriptions::Subscriptions;
    use andromeda_subscription::notifying_since::{NotifyingRestart, SinceRestart};
    use sea_query::Iden;

    fn columns(written: impl IntoIterator<Item = (Subscriptions, sea_query::SimpleExpr)>) -> Vec<String> {
        written.into_iter().map(|(column, _)| column.to_string()).collect()
    }

    #[test]
    fn a_created_subscription_stamps_both_instants() {
        pretty_assertions::assert_eq!(columns(created_since()), vec!["active_since", "selected_since"]);
    }

    #[test]
    fn an_update_writes_only_the_instants_it_restarts() {
        let restart = |active, selected| NotifyingRestart { active, selected };
        pretty_assertions::assert_eq!(columns(restarted_since(restart(SinceRestart::Kept, SinceRestart::Kept))), Vec::<String>::new());
        pretty_assertions::assert_eq!(
            columns(restarted_since(restart(SinceRestart::Restarted, SinceRestart::Kept))),
            vec!["active_since"]
        );
        pretty_assertions::assert_eq!(
            columns(restarted_since(restart(SinceRestart::Kept, SinceRestart::Restarted))),
            vec!["selected_since"]
        );
    }
}
