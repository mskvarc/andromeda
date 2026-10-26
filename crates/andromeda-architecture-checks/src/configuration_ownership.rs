//! The configuration crate is the only owner of configuration types, and the
//! broker composition the only reader of the whole configuration.
//!
//! Two rules keep the schema in one place:
//!
//! - no production source outside `andromeda-configuration` derives
//!   `Deserialize` on a type named `*Config` or `*Settings`, so a section can
//!   only be defined where naming, nesting and completeness are decided;
//! - no production source outside `andromeda-configuration`, the broker
//!   composition and the binary names the root `BrokerConfig`, so a domain
//!   crate receives the one section it reads and never the whole configuration.

use crate::source_scan::{is_test_only, workspace_sources};
use std::{collections::BTreeSet, path::Path};
use syn::{
    Attribute,
    Ident,
    ItemEnum,
    ItemMod,
    ItemStruct,
    Path as SynPath,
    UseName,
    UseRename,
    visit::{Visit, visit_item_enum, visit_item_mod, visit_item_struct, visit_path},
};

/// The crate that owns configuration types.
const CONFIGURATION_CRATE: &str = "crates/andromeda-configuration/";

/// The sources that may name the root configuration: the crate that defines
/// it, the composition that routes its sections, and the binary that loads it.
const ROOT_READERS: [&str; 3] = [CONFIGURATION_CRATE, "crates/andromeda-broker-composition/", "src/"];

/// The root configuration type.
const ROOT_TYPE: &str = "BrokerConfig";

/// Every production site that breaks either rule, as `path: what`, sorted.
#[must_use]
pub fn configuration_ownership_violations(workspace_root: &str) -> Vec<String> {
    workspace_sources(Path::new(workspace_root))
        .into_iter()
        .filter(|source| {
            !source.workspace_path.contains("/tests/") && !source.workspace_path.ends_with("build.rs") && !source.workspace_path.ends_with("tests.rs")
        })
        .flat_map(|source| {
            source_violations(&source.workspace_path, &source.syntax)
                .into_iter()
                .map(move |violation| format!("{}: {violation}", source.workspace_path))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Assert that configuration types are defined in the configuration crate and
/// that the root is read only by the composition.
///
/// # Panics
///
/// Panics on any site [`configuration_ownership_violations`] reports, or when
/// the workspace source tree cannot be read or parsed.
pub fn assert_configuration_has_one_owner(workspace_root: &str) {
    let violations = configuration_ownership_violations(workspace_root);
    assert!(
        violations.is_empty(),
        "configuration types belong to andromeda-configuration, and only the broker composition reads the whole configuration; define the section there, or take the one section the crate reads:\n{}",
        violations.join("\n")
    );
}

/// The violations of the source at `path`, in the order they occur.
fn source_violations(path: &str, syntax: &syn::File) -> Vec<String> {
    let mut visitor = OwnershipVisitor {
        defines_sections: path.starts_with(CONFIGURATION_CRATE),
        reads_root: ROOT_READERS.iter().any(|reader| path.starts_with(reader)),
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

/// Walks one source's production items, recording what breaks either rule.
struct OwnershipVisitor {
    /// Whether the source may define configuration types.
    defines_sections: bool,
    /// Whether the source may name the root configuration.
    reads_root: bool,
    /// What the walk found, in source order.
    violations: Vec<String>,
}

impl OwnershipVisitor {
    /// Record a configuration type `ident` deriving `Deserialize` outside the
    /// configuration crate.
    fn check_definition(&mut self, attributes: &[Attribute], ident: &Ident) {
        let name = ident.to_string();
        if self.defines_sections || !(name.ends_with("Config") || name.ends_with("Settings")) || !derives_deserialize(attributes) {
            return;
        }
        self.violations.push(format!("derives Deserialize on {name}"));
    }

    /// Record a name of the root configuration outside its readers.
    fn check_name(&mut self, ident: &Ident) {
        if !self.reads_root && ident == ROOT_TYPE {
            self.violations.push(format!("names {ROOT_TYPE}"));
        }
    }
}

impl<'ast> Visit<'ast> for OwnershipVisitor {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if !is_test_only(&item.attrs) {
            visit_item_mod(self, item);
        }
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_definition(&item.attrs, &item.ident);
        visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_definition(&item.attrs, &item.ident);
        visit_item_enum(self, item);
    }

    fn visit_path(&mut self, path: &'ast SynPath) {
        path.segments.iter().for_each(|segment| self.check_name(&segment.ident));
        visit_path(self, path);
    }

    fn visit_use_name(&mut self, name: &'ast UseName) {
        self.check_name(&name.ident);
    }

    fn visit_use_rename(&mut self, rename: &'ast UseRename) {
        self.check_name(&rename.ident);
    }
}

/// Whether `attributes` derive `Deserialize`, by its bare or its `serde::` name.
fn derives_deserialize(attributes: &[Attribute]) -> bool {
    attributes.iter().filter(|attribute| attribute.path().is_ident("derive")).any(|attribute| {
        let mut found = false;
        // A derive list that does not parse names nothing this rule reads.
        let _parsed = attribute.parse_nested_meta(|meta| {
            found |= meta.path.segments.last().is_some_and(|segment| segment.ident == "Deserialize");
            Ok(())
        });
        found
    })
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::configuration_ownership::source_violations;
    use pretty_assertions::assert_eq;

    fn violations(path: &str, source: &str) -> Vec<String> {
        source_violations(path, &syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}")))
    }

    const OUTSIDE: &str = "crates/andromeda-entity/src/x.rs";

    #[test]
    fn a_section_deriving_deserialize_outside_the_configuration_crate_is_reported() {
        let source = "#[derive(Debug, serde::Deserialize)]\nstruct EntityConfig { size: usize }\n#[derive(Deserialize)]\nenum RetrySettings { A }\n";
        assert_eq!(
            violations(OUTSIDE, source),
            ["derives Deserialize on EntityConfig", "derives Deserialize on RetrySettings"]
        );
    }

    #[test]
    fn the_configuration_crate_and_other_names_and_tests_are_free() {
        let section = "#[derive(Deserialize)]\nstruct EntityConfig { size: usize }\n";
        assert_eq!(violations("crates/andromeda-configuration/src/schema/entity.rs", section), [] as [String; 0]);
        assert_eq!(
            violations(OUTSIDE, "#[derive(Deserialize)]\nstruct EntityPayload { size: usize }\n"),
            [] as [String; 0]
        );
        assert_eq!(
            violations(OUTSIDE, "#[derive(Serialize)]\nstruct EntityConfig { size: usize }\n"),
            [] as [String; 0]
        );
        assert_eq!(
            violations(
                OUTSIDE,
                "#[cfg(test)]\nmod tests {\n    #[derive(Deserialize)]\n    struct ProbeConfig { a: u8 }\n}\n"
            ),
            [] as [String; 0]
        );
    }

    #[test]
    fn naming_the_root_outside_its_readers_is_reported() {
        let source = "use andromeda_configuration::broker_config::BrokerConfig;\nfn read(config: &BrokerConfig) {}\n";
        assert_eq!(violations(OUTSIDE, source), ["names BrokerConfig", "names BrokerConfig"]);
        assert_eq!(violations("crates/andromeda-broker-composition/src/broker.rs", source), [] as [String; 0]);
        assert_eq!(violations("src/main.rs", source), [] as [String; 0]);
    }
}
