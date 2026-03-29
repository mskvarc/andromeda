pub mod retrieve_available_attribute_information;
pub mod retrieve_available_attributes;
pub mod retrieve_available_entity_type_information;
pub mod retrieve_available_entity_types;
pub mod retrieve_details_of_available_attributes;
pub mod retrieve_details_of_available_entity_types;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    retrieve_available_entity_types::tests(trials);
    retrieve_available_entity_type_information::tests(trials);
    retrieve_available_attributes::tests(trials);
    retrieve_available_attribute_information::tests(trials);
    retrieve_details_of_available_entity_types::tests(trials);
    retrieve_details_of_available_attributes::tests(trials);
}
