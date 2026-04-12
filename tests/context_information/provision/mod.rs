pub mod entities;
pub mod entity_attributes;
pub mod temporal_entity;
pub mod temporal_entity_attributes;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entities::tests(trials);
    entity_attributes::tests(trials);
    temporal_entity::tests(trials);
    temporal_entity_attributes::tests(trials);
}
