mod create;
mod delete;
mod notification;
mod query;
mod retrieve;
mod update;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create::tests(trials);
    retrieve::tests(trials);
    update::tests(trials);
    query::tests(trials);
    delete::tests(trials);
    notification::tests(trials);
}
