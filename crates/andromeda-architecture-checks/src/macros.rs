/// Generate a crate-local Cargo dependency boundary test.
#[macro_export]
macro_rules! dependency_boundary {
    {
        allow_workspace_dependencies: [$($workspace:literal),* $(,)?],
        allow_workspace_dev_dependencies: [$($workspace_dev:literal),* $(,)?],
        allow_production_dependencies: [$($production:literal),* $(,)?] $(,)?
    } => {
        $crate::dependency_boundary! {
            @generate
            allow_workspace_dependencies: [$($workspace),*],
            allow_workspace_dev_dependencies: [$($workspace_dev),*],
            forbidden: &$crate::dependencies::forbidden_production_dependencies_except(&[$($production),*]),
        }
    };
    {
        allow_workspace_dependencies: [$($workspace:literal),* $(,)?],
        allow_workspace_dev_dependencies: [$($workspace_dev:literal),* $(,)?],
        allow_production_dependencies: all $(,)?
    } => {
        $crate::dependency_boundary! {
            @generate
            allow_workspace_dependencies: [$($workspace),*],
            allow_workspace_dev_dependencies: [$($workspace_dev),*],
            forbidden: &[],
        }
    };
    {
        @generate
        allow_workspace_dependencies: [$($workspace:literal),* $(,)?],
        allow_workspace_dev_dependencies: [$($workspace_dev:literal),* $(,)?],
        forbidden: $forbidden:expr $(,)?
    } => {
        #[cfg(test)]
        /// The generated dependency boundary checks for this crate.
        mod architecture_dependency_boundary {
            /// The crate's declared dependencies satisfy its architecture policy.
            #[test]
            fn declared_dependencies_match_architecture_boundary() {
                const ALLOW_WORKSPACE_DEPENDENCIES: &[&str] = &[$($workspace),*];
                const ALLOW_WORKSPACE_DEV_DEPENDENCIES: &[&str] = &[$($workspace_dev),*];
                $crate::dependencies::assert_dependency_boundary(
                    env!("CARGO_MANIFEST_DIR"),
                    $forbidden,
                    ALLOW_WORKSPACE_DEPENDENCIES,
                    ALLOW_WORKSPACE_DEV_DEPENDENCIES,
                );
            }
        }
    };
}

/// Generate a crate-local production-source SQL prohibition test.
#[macro_export]
macro_rules! forbid_sql {
    () => {
        #[cfg(test)]
        /// The generated SQL boundary checks for this crate.
        mod architecture_sql_boundary {
            /// Production source contains no SQL fragments.
            #[test]
            fn production_sources_contain_no_sql_fragments() {
                $crate::sql::assert_no_sql(env!("CARGO_MANIFEST_DIR"));
            }
        }
    };
}

/// Generate the root workspace architecture checks.
#[macro_export]
macro_rules! workspace_architecture {
    {
        workspace_root: $workspace_root:literal,
        no_raw_iri_or_media_constants,
        no_raw_sql,
        retired_architecture $(,)?
    } => {
        #[cfg(test)]
        /// The generated workspace-wide architecture checks.
        mod workspace_architecture_checks {
            /// Production constants do not encode IRIs or media types as strings.
            #[test]
            fn production_constants_do_not_encode_iris_or_media_types_as_strings() {
                $crate::raw_constant::assert_no_raw_iri_or_media_constants(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No source outside the sea-query adapter spells SQL or hands
            /// statement text to the driver.
            #[test]
            fn statements_are_built_through_sea_query_not_hand_written_sql() {
                $crate::raw_sql::assert_no_raw_sql(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Retired architecture surfaces remain absent.
            #[test]
            fn retired_architecture_surfaces_do_not_return() {
                $crate::architecture::assert_retired_architecture_invariants(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No module is named after a dumping ground.
            #[test]
            fn module_names_describe_a_concept() {
                $crate::module_naming::assert_no_dumping_ground_modules(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No production source names a `dyn Error` trait object off the baseline.
            #[test]
            fn errors_are_typed_not_erased() {
                $crate::type_erased_error::assert_no_type_erased_errors(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root),
                    $crate::type_erased_error::TYPE_ERASED_ERROR_BASELINE,
                );
            }

            /// No production source off the baseline swallows a row decode failure.
            #[test]
            fn row_decode_failures_are_propagated_not_swallowed() {
                $crate::swallowed_row_decode::assert_row_decode_failures_are_propagated(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root),
                    $crate::swallowed_row_decode::SWALLOWED_ROW_DECODE_BASELINE,
                );
            }

            /// Every geo-query box test reads the box a stored geometry's index keys.
            #[test]
            fn geometry_box_tests_read_the_indexed_box() {
                $crate::geometry_box_test::assert_geometry_box_tests_read_the_indexed_box(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// The NGSI-LD Scope grammar is read by its one owner only.
            #[test]
            fn scope_grammar_has_one_owner() {
                $crate::scope_grammar::assert_scope_grammar_has_one_owner(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Linked Entity retrieval is decided by its one engine only.
            #[test]
            fn linked_retrieval_has_one_owner() {
                $crate::linked_retrieval::assert_linked_retrieval_has_one_owner(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Registration and subscription matching is decided by the model engine only.
            #[test]
            fn registration_matching_has_one_owner() {
                $crate::registration_matching::assert_registration_matching_has_one_owner(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No production source lets a string stand in for a typed error.
            #[test]
            fn errors_carry_typed_values_not_strings() {
                $crate::string_typed_error::assert_no_string_typed_errors(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Time values are read and written by the lexicon's `time` module only.
            #[test]
            fn time_values_have_one_owner() {
                $crate::time_handling::assert_time_values_have_one_owner(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No production source names an untyped JSON tree.
            #[test]
            fn json_is_held_typed_not_as_an_untyped_tree() {
                $crate::untyped_json::assert_no_untyped_json(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No PostgreSQL crate encodes or decodes an Entity state as its expanded document.
            #[test]
            fn postgresql_holds_entity_state_as_records_not_documents() {
                $crate::state_codec_boundary::assert_no_state_codec_calls(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// No production file crosses the length ceiling off the baseline.
            #[test]
            fn no_production_file_is_a_god_file() {
                $crate::god_file::assert_no_god_files(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root),
                    $crate::god_file::GOD_FILE_BASELINE,
                );
            }

            /// Every manifest pins full versions, stays ordered, and places each dependency kind correctly.
            #[test]
            fn manifests_follow_dependency_hygiene() {
                $crate::manifest_hygiene::assert_manifest_hygiene(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Every package inherits version, edition, publishing and lints from the workspace and describes itself.
            #[test]
            fn manifests_inherit_package_metadata() {
                $crate::package_metadata::assert_package_metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Configuration types are defined in the configuration crate, and only the composition reads the root.
            #[test]
            fn configuration_has_one_owner() {
                $crate::configuration_ownership::assert_configuration_has_one_owner(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Every configuration type, key and choice carries an operator description.
            #[test]
            fn configuration_is_described_for_the_operator() {
                $crate::configuration_description::assert_configuration_is_described(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// The configuration schema crate depends on no other in-tree crate.
            #[test]
            fn configuration_is_the_bottom_of_the_graph() {
                $crate::configuration_layering::assert_configuration_is_a_leaf(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Client-facing error detail is lowercase prose that interpolates through Display.
            #[test]
            fn error_detail_messages_are_well_formed() {
                $crate::detail_message::assert_detail_messages_are_well_formed(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root),
                    $crate::detail_message_baseline::DETAIL_MESSAGE_BASELINE,
                );
            }
        }
    };
}
