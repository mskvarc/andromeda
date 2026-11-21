//! The advisory rulings the broker image's VEX document publishes agree with
//! the ones `deny.toml` records.
//!
//! `deny.toml` ignores an advisory with the reason the broker is not affected
//! by it. The OpenVEX document attested beside the image tells scanners the
//! same ruling, as a `not_affected` statement whose impact statement is that
//! reason, about the crate version the image links. The gate holds every
//! statement to an ignore of its advisory with the same reason, and every crate
//! a statement names to a version `Cargo.lock` holds, so a ruling cannot change,
//! lapse or move to another crate version in one place alone. A ruling that a
//! crate's code never runs rests on what the crates linking it call, so the
//! impact statement names each of them with its version, and the gate fails
//! once the lockfile adds or moves one, because the ruling then has to be
//! proven again for it. Each product is
//! the broker image's package URL without a version, under the owner of the
//! workspace's repository, the form a scanner matches every digest of the image
//! by.
//!
//! An ignore needs a statement only when scanners report its advisory, which
//! the advisory databases decide and the workspace does not record, so an
//! ignore without a statement passes.

mod advisory_ignores;
mod broker_image;
mod disagreement;
mod locked_crates;
mod vex_document;

use crate::{
    advisory_rulings::{
        advisory_ignores::advisory_ignores,
        broker_image::BrokerImage,
        disagreement::Disagreement,
        locked_crates::LockedCrates,
        vex_document::{Product, Statement, Status, VexDocument},
    },
    workspace_file::{read_toml_document, read_workspace_file},
};
use packageurl::PackageUrl;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    str::FromStr,
};

/// The OpenVEX document the release attests to the broker image, relative to
/// the workspace root.
const VEX_DOCUMENT: &str = ".vex/andromeda.openvex.json";

/// The cargo-deny configuration, relative to the workspace root.
const DENY_CONFIGURATION: &str = "deny.toml";

/// The workspace lockfile, relative to the workspace root.
const LOCKFILE: &str = "Cargo.lock";

/// The root manifest, whose `[workspace.package]` repository names the owner
/// the broker image is published under, relative to the workspace root.
const ROOT_MANIFEST: &str = "Cargo.toml";

/// The package URL type of a crate.
const CRATE_TYPE: &str = "cargo";

/// Assert that every statement of the broker image's VEX document repeats an
/// advisory ruling of `deny.toml` for a crate version `Cargo.lock` holds.
///
/// # Panics
///
/// Panics when a statement disagrees with `deny.toml` or `Cargo.lock`, when a
/// product or crate it names is not spelt as a scanner matches it, when the
/// root manifest names no GitHub repository, or when one of the four files
/// cannot be read or parsed.
pub fn assert_vex_statements_repeat_advisory_rulings(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    let ignores = read_toml_document(root, DENY_CONFIGURATION, &mut violations).map(|document| advisory_ignores(&document));
    let locked = read_toml_document(root, LOCKFILE, &mut violations).map(|document| LockedCrates::from_document(&document));
    let image = read_broker_image(root, &mut violations);
    let document = read_vex_document(root, &mut violations);
    if let (Some(ignores), Some(locked), Some(image), Some(document)) = (ignores, locked, image, document) {
        violations.extend(disagreements(&document, &ignores, &locked, &image).iter().map(ToString::to_string));
    }
    assert!(
        violations.is_empty(),
        "VEX statements that do not repeat an advisory ruling:\n{}",
        violations.join("\n")
    );
}

/// The broker image the root manifest's repository owner publishes, or `None`
/// with the reason recorded in `violations`.
fn read_broker_image(root: &Path, violations: &mut Vec<String>) -> Option<BrokerImage> {
    let manifest = read_toml_document(root, ROOT_MANIFEST, violations)?;
    let image = BrokerImage::from_manifest(&manifest);
    if image.is_none() {
        violations.push(format!(
            "{ROOT_MANIFEST} declares no GitHub repository in [workspace.package], whose owner the broker image is published under"
        ));
    }
    image
}

/// The VEX document, or `None` with the reason recorded in `violations`.
fn read_vex_document(root: &Path, violations: &mut Vec<String>) -> Option<VexDocument> {
    let text = read_workspace_file(root, VEX_DOCUMENT, violations)?;
    match serde_json::from_str(&text) {
        Ok(document) => Some(document),
        Err(error) => {
            violations.push(format!("{VEX_DOCUMENT} does not parse as an OpenVEX document: {error}"));
            None
        }
    }
}

/// Every way a statement of `document` disagrees with the advisories `ignores`
/// records, the crates `locked` holds or the broker `image`, in statement order.
fn disagreements<'a>(
    document: &'a VexDocument,
    ignores: &BTreeMap<String, Option<String>>,
    locked: &'a LockedCrates,
    image: &'a BrokerImage,
) -> Vec<Disagreement<'a>> {
    let mut ruled = BTreeSet::new();
    document
        .statements
        .iter()
        .flat_map(|statement| {
            let advisory = statement.vulnerability.name.as_str();
            let repeated = (!ruled.insert(advisory)).then_some(Disagreement::Repeated { advisory });
            repeated
                .into_iter()
                .chain(ruling_disagreements(statement, ignores))
                .chain(product_disagreements(statement, locked, image))
        })
        .collect()
}

/// How `statement` disagrees with the ruling `ignores` records for its advisory.
fn ruling_disagreements<'a>(statement: &'a Statement, ignores: &BTreeMap<String, Option<String>>) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    let ignore = match ignores.get(advisory) {
        None => Some(Disagreement::NotIgnored { advisory }),
        Some(None) => Some(Disagreement::IgnoredWithoutReason { advisory }),
        Some(Some(reason)) => (statement.impact.as_deref() != Some(reason.as_str())).then_some(Disagreement::ImpactDiffers { advisory }),
    };
    let status = (statement.status != Status::NotAffected).then_some(Disagreement::NotNotAffected {
        advisory,
        status: statement.status,
    });
    let justification = statement.justification.is_none().then_some(Disagreement::Unjustified { advisory });
    [ignore, status, justification].into_iter().flatten().collect()
}

/// How the products of `statement`, and the crates inside them, disagree with
/// the broker `image` and the crates `locked` holds.
fn product_disagreements<'a>(statement: &'a Statement, locked: &'a LockedCrates, image: &'a BrokerImage) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    if statement.products.is_empty() {
        return vec![Disagreement::NoProduct { advisory }];
    }
    statement
        .products
        .iter()
        .flat_map(|product| {
            let not_image = (!image.is_named_by(&product.id)).then_some(Disagreement::ProductNotImage {
                advisory,
                product: &product.id,
                image: image.purl(),
            });
            not_image.into_iter().chain(subcomponent_disagreements(statement, product, locked))
        })
        .collect()
}

/// How the crates inside `product` disagree with the crates `locked` holds.
fn subcomponent_disagreements<'a>(statement: &'a Statement, product: &'a Product, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    if product.subcomponents.is_empty() {
        return vec![Disagreement::NoSubcomponent {
            advisory,
            product: &product.id,
        }];
    }
    product
        .subcomponents
        .iter()
        .flat_map(|subcomponent| crate_disagreements(statement, &subcomponent.id, locked))
        .collect()
}

/// How the crate version `subcomponent` names disagrees with the crates
/// `locked` holds: it is no crate version, the lockfile does not hold it, or a
/// crate linking it goes unnamed in the impact statement of `statement`.
fn crate_disagreements<'a>(statement: &'a Statement, subcomponent: &'a str, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    let purl = PackageUrl::from_str(subcomponent).ok().filter(|purl| purl.ty() == CRATE_TYPE);
    let Some((name, version)) = purl.as_ref().and_then(|purl| Some((purl.name(), purl.version()?))) else {
        return vec![Disagreement::SubcomponentNotCrate { advisory, subcomponent }];
    };
    if !locked.holds(name, version) {
        return vec![Disagreement::NotLocked { advisory, subcomponent }];
    }
    let impact = statement.impact.as_deref().unwrap_or_default();
    locked
        .dependents(name, version)
        .filter(|dependent| !names_crate_version(impact, &dependent.name, &dependent.version))
        .map(|dependent| Disagreement::DependentUnnamed {
            advisory,
            subcomponent,
            dependent: &dependent.name,
            version: &dependent.version,
        })
        .collect()
}

/// Whether `text` names the crate `name` at `version` as the words
/// `<name> <version>`, with neither run on into a longer name or version.
fn names_crate_version(text: &str, name: &str, version: &str) -> bool {
    let words = format!("{name} {version}");
    text.match_indices(&words).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + words.len()..].chars().next();
        !before.is_some_and(|character| character.is_alphanumeric() || character == '-' || character == '_') && !after.is_some_and(char::is_alphanumeric)
    })
}

#[cfg(test)]
mod tests;
