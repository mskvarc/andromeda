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

            /// No unsanctioned, non-baseline source builds SQL by hand.
            #[test]
            fn statements_are_built_through_sea_query_not_hand_written_sql() {
                $crate::raw_sql::assert_no_new_raw_sql(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root), $crate::raw_sql::RAW_SQL_BASELINE);
            }

            /// Retired architecture surfaces remain absent.
            #[test]
            fn retired_architecture_surfaces_do_not_return() {
                $crate::architecture::assert_retired_architecture_invariants(concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root));
            }

            /// Focused test infrastructure retains its source and dependency boundaries.
            #[test]
            fn test_infrastructure_boundaries_remain_explicit() {
                let workspace_root = concat!(env!("CARGO_MANIFEST_DIR"), "/", $workspace_root);
                $crate::architecture::assert_test_infrastructure_invariants(workspace_root);
                $crate::dependencies::assert_test_infrastructure_is_development_only(workspace_root);
            }
        }
    };
}
