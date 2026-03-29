pub mod create_subscription;
pub mod delete_subscription;
pub mod notification_behaviour;
pub mod query_subscriptions;
pub mod retrieve_subscription;
pub mod update_subscription;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create_subscription::tests(trials);
    delete_subscription::tests(trials);
    retrieve_subscription::tests(trials);
    update_subscription::tests(trials);
    query_subscriptions::tests(trials);
    notification_behaviour::tests(trials);
}
