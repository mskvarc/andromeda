pub mod source_identity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    source_identity::tests(trials);
}
