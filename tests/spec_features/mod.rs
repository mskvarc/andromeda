pub mod extension_headers;
pub mod notification_join_level;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    notification_join_level::tests(trials);
    extension_headers::tests(trials);
}
