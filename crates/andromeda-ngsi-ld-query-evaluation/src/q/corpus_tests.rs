//! Every filter of the parity corpus, evaluated in memory over the corpus
//! Entities, each in a test of its own.
//!
//! The corpus names the Entities clause 4.9 has each filter select, and every
//! case asserts that selection.

use crate::{
    q::filter_run::filter_holds,
    queried_entity::{corpus_fixture::corpus_entities, expanded_fixture::FixtureLinked},
};
use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, reading::filter_reading::FilterReading};
use andromeda_q_corpus::{case_filter::expanded_filter, corpus::CORPUS, corpus_entity::CorpusEntity};
use pretty_assertions::assert_eq;
use strum::IntoEnumIterator;

/// The corpus Entities the filter of the case named `id` selects, or the
/// refusal of its filter.
async fn selected(id: &str) -> Result<Vec<CorpusEntity>, String> {
    let case = CORPUS.iter().find(|case| case.id == id).expect("the case is in the corpus");
    let expr = expanded_filter(case).await.map_err(|refusal| refusal.to_string())?;
    let reading = FilterReading::of(&expr, LanguageMatching::TagEquality);
    let entities = corpus_entities();
    Ok(CorpusEntity::iter()
        .zip(&entities)
        .filter(|(_, entity)| filter_holds(&reading, *entity, FixtureLinked(&entities)))
        .map(|(corpus_entity, _)| corpus_entity)
        .collect())
}

/// Hold the case named `id` to the selection the corpus names.
async fn assert_case(id: &str) {
    let case = CORPUS.iter().find(|case| case.id == id).expect("the case is in the corpus");
    let mut expected = case.selects.to_vec();
    expected.sort();
    let selection = selected(id).await;
    assert_eq!(selection, Ok(expected), "{id} {}", case.q);
}

/// Generates one test per corpus case, each named for what the case pins.
macro_rules! corpus_cases {
    ($($test:ident => $case:literal),+ $(,)?) => {$(
        #[tokio::test]
        async fn $test() {
            assert_case($case).await;
        }
    )+};
}

corpus_cases! {
    c_01_a_number_equality_selects_a_scalar_an_array_member_and_any_instance => "C-01",
    c_02_a_number_inequality_selects_a_string_and_an_unequal_instance => "C-02",
    c_03_greater_than_skips_an_array_and_selects_through_any_instance => "C-03",
    c_04_less_or_equal_selects_through_any_instance => "C-04",
    c_05_an_inclusive_range_selects_through_any_instance => "C-05",
    c_06_an_excluded_range_selects_a_string_an_array_and_an_instance_outside => "C-06",
    c_07_a_value_list_selects_a_scalar_an_array_member_and_any_instance => "C-07",
    c_08_an_excluded_value_list_skips_an_array_holding_a_member => "C-08",
    c_09_existence_selects_every_holder_of_the_attribute => "C-09",
    c_10_a_string_equality_selects_the_equal_string => "C-10",
    c_11_a_string_inequality_selects_a_number_as_unequal => "C-11",
    c_12_a_string_ordering_skips_a_number => "C-12",
    c_13_a_pattern_selects_matching_strings => "C-13",
    c_14_a_negated_pattern_skips_a_number => "C-14",
    c_15_a_boolean_equality_selects_true => "C-15",
    c_16_a_boolean_inequality_selects_false => "C-16",
    c_17_a_quoted_uri_selects_the_relationship_object => "C-17",
    c_18_an_ordering_never_selects_a_relationship => "C-18",
    c_19_a_pattern_never_selects_a_relationship => "C-19",
    c_20_an_inequality_selects_a_relationship_to_another_object => "C-20",
    c_21_an_equality_selects_a_list_relationship_holding_the_object => "C-21",
    c_22_an_inequality_skips_a_list_relationship_holding_the_object => "C-22",
    c_23_an_equality_selects_any_string_of_a_language_map => "C-23",
    c_24_an_inequality_skips_a_language_map_holding_the_string => "C-24",
    c_25_an_inequality_selects_a_language_map_lacking_the_string => "C-25",
    c_26_a_target_language_selects_its_string => "C-26",
    c_27_a_target_language_skips_another_languages_string => "C-27",
    c_28_an_absent_target_language_is_unequal => "C-28",
    c_29_a_pattern_reads_any_string_of_a_language_map => "C-29",
    c_30_a_negated_pattern_selects_a_language_map_with_no_match => "C-30",
    c_31_a_negated_pattern_searches_anywhere_in_a_language_map_string => "C-31",
    c_32_a_date_operand_selects_a_declared_date => "C-32",
    c_33_a_quoted_operand_never_equals_a_declared_date => "C-33",
    c_34_a_quoted_operand_is_unequal_to_a_declared_date => "C-34",
    c_35_a_pattern_skips_a_declared_date => "C-35",
    c_36_a_date_time_ordering_selects_a_later_declared_date_time => "C-36",
    c_37_not_a_number_is_unordered => "C-37",
    c_38_not_a_number_is_unequal_to_every_number => "C-38",
    c_39_an_infinity_orders_past_every_finite_number => "C-39",
    c_40_an_infinity_equals_no_finite_number => "C-40",
    c_41_observed_at_selects_by_the_instance_member => "C-41",
    c_42_unit_code_selects_by_the_instance_member => "C-42",
    c_43_a_pattern_reads_the_unit_code => "C-43",
    c_44_a_sub_property_equality_selects_its_holder => "C-44",
    c_45_a_sub_property_existence_selects_its_holder => "C-45",
    c_46_an_absent_sub_property_selects_nothing => "C-46",
    c_47_a_compound_member_equality_selects_its_holder => "C-47",
    c_48_a_compound_member_ordering_selects_its_holder => "C-48",
    c_49_a_conjunction_selects_only_what_both_terms_select => "C-49",
    c_50_a_disjunction_selects_what_either_term_selects => "C-50",
    c_51_an_absent_compound_member_selects_nothing => "C-51",
    c_52_an_inequality_on_an_absent_compound_member_selects_nothing => "C-52",
    c_53_a_relationship_is_unequal_to_a_range => "C-53",
    c_54_a_string_ordering_reads_any_string_of_a_language_map => "C-54",
    c_55_an_excluded_string_range_selects_a_language_map_with_a_string_outside => "C-55",
    c_56_a_uri_operand_selects_a_relationship_in_another_spelling => "C-56",
    c_57_expand_values_expands_the_operand_of_a_vocab_property => "C-57",
    c_58_an_unexpanded_operand_never_equals_a_vocab_property => "C-58",
    c_59_a_list_property_includes_the_operand => "C-59",
    c_60_an_ordering_never_selects_a_list_property => "C-60",
    c_61_json_keys_keeps_a_json_property_member_literal => "C-61",
    c_62_a_member_reaches_into_a_geo_property => "C-62",
    c_63_a_string_property_compares_with_the_uri_spelling_written => "C-63",
    c_64_a_value_list_member_selects_a_relationship_in_another_spelling => "C-64",
    c_65_a_hop_reads_the_linked_entity => "C-65",
    c_66_a_hop_inequality_skips_a_linked_entity_holding_the_value => "C-66",
    c_67_a_hop_to_an_entity_lacking_the_attribute_selects_nothing => "C-67",
    c_68_a_hop_guard_admits_the_named_type => "C-68",
    c_69_a_hop_guard_skips_another_type => "C-69",
    c_70_a_one_element_array_reads_as_its_element => "C-70",
    c_71_a_date_time_equality_compares_instants => "C-71",
    c_72_a_time_ordering_compares_times => "C-72",
    c_73_a_comma_before_the_fraction_is_a_date_time => "C-73",
    c_74_an_inequality_on_an_absent_sub_property_selects_nothing => "C-74",
    c_75_a_negated_pattern_skips_a_declared_date => "C-75",
    c_76_a_list_relationship_is_unequal_to_a_range => "C-76",
    c_77_a_relationship_is_unequal_to_a_string_range => "C-77",
    c_78_a_structured_value_is_unequal_to_a_string => "C-78",
    c_79_a_geometry_is_unequal_to_a_string => "C-79",
    c_80_a_hop_crosses_a_list_relationship => "C-80",
    c_81_a_uri_matches_one_of_several_objects_in_its_normal_form => "C-81",
    c_82_an_element_of_an_array_member_matches => "C-82",
    c_83_an_element_of_a_nested_array_is_no_element_of_the_member => "C-83",
    c_84_a_nested_array_includes_no_operand => "C-84",
    c_85_an_empty_array_is_unequal_to_a_string => "C-85",
    c_86_an_empty_array_includes_no_operand => "C-86",
    c_87_an_empty_array_value_defines_the_attribute => "C-87",
    c_88_a_json_property_is_read_by_its_own_keys => "C-88",
    c_89_a_json_property_holds_no_key_its_literal_does_not_write => "C-89",
    c_90_a_coordinate_compares_as_its_shortest_spelling => "C-90",
    c_91_a_longitude_compares_as_its_shortest_spelling => "C-91",
    c_92_a_json_property_holding_a_literal_shaped_object_is_present => "C-92",
    c_93_a_compound_member_holding_the_empty_array_is_present => "C-93",
    c_94_a_compound_member_holding_the_empty_array_is_unequal_to_a_string => "C-94",
    c_95_a_compound_member_holding_the_empty_array_includes_no_operand => "C-95",
    c_96_a_structured_compound_member_is_unequal_to_a_string => "C-96",
    c_97_an_element_of_a_compound_member_beside_a_structured_value_matches => "C-97",
    c_98_an_ordering_never_selects_a_compound_member_holding_an_array => "C-98",
    c_99_a_compound_member_declared_a_year_zero_date_time_precedes_year_one => "C-99",
    c_100_a_compound_member_declared_a_year_zero_date_time_equals_its_instant => "C-100",
    c_101_a_geometry_holds_its_bbox_as_a_member => "C-101",
    c_102_a_bbox_includes_each_of_its_bounds => "C-102",
    c_103_an_ordering_never_selects_a_bbox => "C-103",
    c_104_a_bbox_holding_the_operand_is_not_unequal_to_it => "C-104",
    c_105_a_foreign_member_is_read_by_its_name => "C-105",
    c_106_a_foreign_member_holding_another_string_is_not_equal => "C-106",
    c_107_a_member_of_a_foreign_member_is_reached => "C-107",
    c_108_a_structured_foreign_member_is_unequal_to_a_string => "C-108",
    c_109_a_geometry_holds_no_member_it_does_not_write => "C-109",
    c_110_a_geometry_written_without_a_bbox_holds_none => "C-110",
    c_111_a_trailing_path_reaches_no_member_of_several_structured_values => "C-111",
    c_112_a_member_of_the_first_of_several_structured_values_is_no_target => "C-112",
    c_113_a_member_of_the_second_of_several_structured_values_is_no_target => "C-113",
    c_114_a_value_list_reads_a_member_of_a_single_structured_value_alone => "C-114",
    c_115_an_inequality_never_selects_several_structured_values => "C-115",
    c_116_a_bare_pattern_selects_as_the_quoted_one => "C-116",
    c_117_a_bare_negated_pattern_selects_as_the_quoted_one => "C-117",
    c_118_a_bare_pattern_keeps_the_alternation_inside_its_group => "C-118",
    c_119_a_bare_pattern_ends_at_the_parenthesis_closing_its_group_of_terms => "C-119",
    c_120_a_bare_pattern_ends_at_the_disjunction_that_joins_the_next_term => "C-120",
}
