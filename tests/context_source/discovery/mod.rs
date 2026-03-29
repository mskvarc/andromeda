mod query_registrations;
mod retrieve_registration;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    retrieve_registration::tests(trials);
    query_registrations::tests(trials);
}
