pub mod batch_entities;
pub mod entities;
pub mod entity_attributes;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entities::tests(trials);
    entity_attributes::tests(trials);
    batch_entities::tests(trials);
}
