pub mod consumption;
pub mod provision;
pub mod subscription;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    provision::tests(trials);
    consumption::tests(trials);
    subscription::tests(trials);
}
