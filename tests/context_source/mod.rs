pub mod discovery;
pub mod registration;
pub mod registration_subscription;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    discovery::tests(trials);
    registration::tests(trials);
    registration_subscription::tests(trials);
}
