pub mod discovery;
pub mod entity;
pub mod temporal_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entity::tests(trials);
    temporal_entity::tests(trials);
    discovery::tests(trials);
}
