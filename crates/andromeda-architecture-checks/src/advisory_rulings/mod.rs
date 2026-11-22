//! The broker image's VEX document is generated from the advisory rulings, and
//! the rulings agree with `deny.toml` and `Cargo.lock`.
//!
//! `deny.toml` ignores an advisory with the reason the broker is not affected
//! by it. The rulings file gives, for each ignored advisory, its other
//! identifiers, the OpenVEX justification and the crate versions the ruling is
//! about. The OpenVEX document attested beside the image is generated from the
//! two and checked in, so scanners read every ruling cargo-deny applies, under
//! every identifier a scanner may report the advisory by, with the reason as
//! the impact statement. A checked-in document that differs from the generated
//! one fails the gate, and running it with `ANDROMEDA_REGENERATE=1` writes it.
//!
//! The gate holds every ignore to a ruling and to a reason, every ruling to an
//! ignore, and every crate version a ruling names to one `Cargo.lock` holds. A
//! ruling that a crate's code never runs, or runs only on input no adversary
//! controls, rests on what the crates linking it call, so its reason names
//! each of them with its version, and the gate fails once the lockfile adds or
//! moves one, because the ruling then has to be proven again for it. A reason
//! that relies on a fixed release of a crate the broker uses names it as
//! `<name> <version> or later`. An ignore covers every version of the crate,
//! so the gate fails once the lockfile holds no version that high, or one
//! below it the ruling is not about.

mod advisory_ignores;
mod broker_image;
mod disagreement;
mod impact_text;
mod locked_crates;
mod rendering;
mod rulings_file;
mod source_repository;
mod vex_document;

use crate::{
    advisory_rulings::{
        advisory_ignores::advisory_ignores,
        disagreement::Disagreement,
        impact_text::{names_crate_version, version_floors},
        locked_crates::LockedCrates,
        rendering::document_text,
        rulings_file::{Ruling, read_rulings},
        source_repository::SourceRepository,
    },
    generated_artifact::assert_checked_in,
    workspace_file::read_toml_document,
};
use std::{collections::BTreeMap, fs, path::Path};

/// The OpenVEX document the release attests to the broker image, relative to
/// the workspace root.
const VEX_DOCUMENT: &str = ".vex/andromeda.openvex.json";

/// The rulings file, relative to the workspace root.
const RULINGS: &str = ".vex/rulings.toml";

/// The cargo-deny configuration, relative to the workspace root.
const DENY_CONFIGURATION: &str = "deny.toml";

/// The workspace lockfile, relative to the workspace root.
const LOCKFILE: &str = "Cargo.lock";

/// The root manifest, whose `[workspace.package]` repository names the owner
/// the broker image is published under, relative to the workspace root.
const ROOT_MANIFEST: &str = "Cargo.toml";

/// Assert that the rulings agree with `deny.toml` and `Cargo.lock`, and that
/// the checked-in VEX document is the one generated from them, or write it
/// when regenerating.
///
/// # Panics
///
/// Panics when a ruling or an ignore disagrees, when the checked-in document
/// differs from the generated one, when the root manifest names no GitHub
/// repository, or when one of the files cannot be read or parsed.
pub fn assert_vex_document_is_generated_from_advisory_rulings(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    let ignores = read_toml_document(root, DENY_CONFIGURATION, &mut violations).map(|document| advisory_ignores(&document));
    let rulings = read_toml_document(root, RULINGS, &mut violations).map(|document| read_rulings(&document, RULINGS, &mut violations));
    let locked = read_toml_document(root, LOCKFILE, &mut violations).map(|document| LockedCrates::from_document(&document));
    let source = read_source_repository(root, &mut violations);
    let (Some(ignores), Some(rulings), Some(locked), Some(source)) = (ignores, rulings, locked, source) else {
        assert_agreement(&violations);
        return;
    };
    violations.extend(disagreements(&ignores, &rulings, &locked).iter().map(ToString::to_string));
    assert_agreement(&violations);
    let checked_in = fs::read_to_string(root.join(VEX_DOCUMENT)).ok();
    let generated = document_text(&rulings, &ignores, &source, VEX_DOCUMENT, checked_in.as_deref());
    assert!(generated.is_ok(), "{VEX_DOCUMENT} cannot be generated: {generated:?}");
    if let Ok(generated) = generated {
        assert_checked_in(workspace_root, VEX_DOCUMENT, &generated);
    }
}

/// Assert that nothing disagrees.
fn assert_agreement(violations: &[String]) {
    assert!(
        violations.is_empty(),
        "the advisory rulings do not agree with deny.toml or Cargo.lock:\n{}",
        violations.join("\n")
    );
}

/// The repository the root manifest names, or `None` with the reason recorded
/// in `violations`.
fn read_source_repository(root: &Path, violations: &mut Vec<String>) -> Option<SourceRepository> {
    let manifest = read_toml_document(root, ROOT_MANIFEST, violations)?;
    let source = SourceRepository::from_manifest(&manifest);
    if source.is_none() {
        violations.push(format!(
            "{ROOT_MANIFEST} declares no GitHub repository in [workspace.package], whose owner the broker image is published under"
        ));
    }
    source
}

/// Every way the advisories `ignores` records, the `rulings` and the crates
/// `locked` holds disagree: ignores first, then rulings, each in advisory
/// order, then every identifier named more than once.
fn disagreements<'a>(ignores: &'a BTreeMap<String, Option<String>>, rulings: &'a BTreeMap<String, Ruling>, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    let of_ignores = ignores.iter().flat_map(|(advisory, reason)| {
        let unruled = (!rulings.contains_key(advisory)).then_some(Disagreement::IgnoreWithoutRuling { advisory });
        let unreasoned = reason.is_none().then_some(Disagreement::IgnoredWithoutReason { advisory });
        unruled.into_iter().chain(unreasoned)
    });
    let of_rulings = rulings.iter().flat_map(|(advisory, ruling)| {
        let unignored = (!ignores.contains_key(advisory)).then_some(Disagreement::RulingNotIgnored { advisory });
        let reason = ignores.get(advisory).and_then(Option::as_deref);
        unignored
            .into_iter()
            .chain(crate_disagreements(advisory, ruling, reason, locked))
            .chain(reason.into_iter().flat_map(|reason| floor_disagreements(advisory, ruling, reason, locked)))
    });
    of_ignores.chain(of_rulings).chain(repeated_identifiers(rulings)).collect()
}

/// How the crate versions `ruling` is about disagree with the crates `locked`
/// holds: the lockfile does not hold one, or, for a ruling that rests on what
/// the linking crates call, a crate linking it goes unnamed in `reason`.
fn crate_disagreements<'a>(advisory: &'a str, ruling: &'a Ruling, reason: Option<&'a str>, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    let callers_reason = reason.filter(|_| ruling.justification.rests_on_callers());
    ruling
        .crates
        .iter()
        .flat_map(|crate_version| {
            let (name, version) = (crate_version.name.as_str(), crate_version.version.as_str());
            if !locked.holds(name, version) {
                return vec![Disagreement::NotLocked { advisory, name, version }];
            }
            let Some(reason) = callers_reason else {
                return Vec::new();
            };
            locked
                .dependents(name, version)
                .filter(|dependent| !names_crate_version(reason, &dependent.name, &dependent.version))
                .map(|dependent| Disagreement::DependentUnnamed {
                    advisory,
                    name,
                    version,
                    dependent: &dependent.name,
                    dependent_version: &dependent.version,
                })
                .collect()
        })
        .collect()
}

/// How the crate versions `locked` holds disagree with each version floor
/// `reason` relies on: no locked version of the crate reaches the floor, or
/// one below it is no crate version `ruling` is about.
fn floor_disagreements<'a>(advisory: &'a str, ruling: &'a Ruling, reason: &'a str, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    version_floors(reason)
        .into_iter()
        .flat_map(|floor| {
            let unreached = (!locked.versions(floor.name).any(|version| floor.admits(version))).then_some(Disagreement::FloorNotLocked {
                advisory,
                name: floor.name,
                floor: floor.spelt,
            });
            let unruled = locked
                .versions(floor.name)
                .filter(|version| {
                    !floor.admits(version)
                        && !ruling
                            .crates
                            .iter()
                            .any(|crate_version| crate_version.name == floor.name && crate_version.version == *version)
                })
                .map(|version| Disagreement::BelowFloor {
                    advisory,
                    name: floor.name,
                    version,
                    floor: floor.spelt,
                })
                .collect::<Vec<_>>();
            unreached.into_iter().chain(unruled)
        })
        .collect()
}

/// Every identifier more than one ruling, or one ruling more than once, names,
/// in sorted order.
fn repeated_identifiers(rulings: &BTreeMap<String, Ruling>) -> Vec<Disagreement<'_>> {
    let counts = rulings
        .values()
        .flat_map(|ruling| &ruling.identifiers)
        .fold(BTreeMap::<&str, usize>::new(), |mut counts, identifier| {
            *counts.entry(identifier.as_str()).or_default() += 1;
            counts
        });
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(identifier, _)| Disagreement::IdentifierRepeated { identifier })
        .collect()
}

#[cfg(test)]
mod tests;
