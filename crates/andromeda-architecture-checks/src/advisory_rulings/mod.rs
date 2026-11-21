//! The advisory rulings the broker image's VEX document publishes agree with
//! the ones `deny.toml` records.
//!
//! `deny.toml` ignores an advisory with the reason the broker is not affected
//! by it. The OpenVEX document attested beside the image tells scanners the
//! same ruling, as a `not_affected` statement whose impact statement is that
//! reason, about the crate version the image links. Scanners that read the
//! Rust advisory database report every advisory cargo-deny reports, so every
//! ignore needs a statement. A scanner reports an advisory under one of its
//! identifiers, the Rust advisory, CVE or GitHub one, and some apply only a
//! statement named by the identifier they report, so an advisory gets one
//! statement per identifier, each listing the others as aliases and all ruling
//! alike.
//!
//! The gate holds every ignore to a statement, every statement to an ignore of
//! one of its identifiers with the same reason, the statements of one advisory
//! to the same identifiers, justification and crates, and every crate a
//! statement names to a version `Cargo.lock` holds, so a ruling cannot change,
//! lapse or move to another crate version in one place alone. A ruling that a
//! crate's code never runs, or runs only on input no adversary controls, rests
//! on what the crates linking it call, so its impact statement names each of
//! them with its version, and the gate fails once the lockfile adds or moves
//! one, because the ruling then has to be proven again for it. Each statement
//! names the broker image in every form scanners match every digest of it by,
//! under the owner of the workspace's repository, and carries its own
//! timestamp, so a later change elsewhere in the document leaves its date as
//! it was.

mod advisory_ignores;
mod broker_image;
mod disagreement;
mod locked_crates;
mod vex_document;

use crate::{
    advisory_rulings::{
        advisory_ignores::advisory_ignores,
        broker_image::{BrokerImage, ImageForm},
        disagreement::Disagreement,
        locked_crates::LockedCrates,
        vex_document::{Justification, Statement, Status, VexDocument},
    },
    workspace_file::{read_toml_document, read_workspace_file},
};
use packageurl::PackageUrl;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    ptr,
    str::FromStr,
};
use strum::IntoEnumIterator;

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

/// Assert that the broker image's VEX document states every advisory ruling of
/// `deny.toml`, once under each identifier of the advisory, for crate versions
/// `Cargo.lock` holds.
///
/// # Panics
///
/// Panics when an ignore has no statement, when a statement disagrees with
/// `deny.toml`, `Cargo.lock` or the other statements of its advisory, when a
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
/// records, the crates `locked` holds, the broker `image` or the other
/// statements of its advisory, in statement order, followed by every ignore no
/// statement names.
fn disagreements<'a>(
    document: &'a VexDocument,
    ignores: &'a BTreeMap<String, Option<String>>,
    locked: &'a LockedCrates,
    image: &'a BrokerImage,
) -> Vec<Disagreement<'a>> {
    let named = first_statement_by_name(document);
    let stated = document.statements.iter().flat_map(|statement| {
        let advisory = statement.vulnerability.name.as_str();
        let repeated = named
            .get(advisory)
            .is_some_and(|first| !ptr::eq(*first, statement))
            .then_some(Disagreement::Repeated { advisory });
        repeated
            .into_iter()
            .chain(ruling_disagreements(statement, ignores))
            .chain(alias_disagreements(statement, &named))
            .chain(product_disagreements(statement, image))
            .chain(crate_disagreements(statement, locked))
    });
    let unstated = ignores
        .keys()
        .filter(|ignored| {
            !document
                .statements
                .iter()
                .any(|statement| statement.vulnerability.identifiers().contains(ignored.as_str()))
        })
        .map(|ignored| Disagreement::IgnoreWithoutStatement { advisory: ignored });
    stated.chain(unstated).collect()
}

/// The first statement of `document` named by each identifier.
fn first_statement_by_name(document: &VexDocument) -> BTreeMap<&str, &Statement> {
    document.statements.iter().fold(BTreeMap::new(), |mut named, statement| {
        named.entry(statement.vulnerability.name.as_str()).or_insert(statement);
        named
    })
}

/// How `statement` disagrees with the ruling `ignores` records for the first
/// of its identifiers, in sorted order, that `deny.toml` ignores.
fn ruling_disagreements<'a>(statement: &'a Statement, ignores: &'a BTreeMap<String, Option<String>>) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    let ignore = match statement
        .vulnerability
        .identifiers()
        .into_iter()
        .find_map(|identifier| ignores.get_key_value(identifier))
    {
        None => Some(Disagreement::NotIgnored { advisory }),
        Some((identifier, None)) => Some(Disagreement::IgnoredWithoutReason { advisory, ignored: identifier }),
        Some((identifier, Some(reason))) => {
            (statement.impact.as_deref() != Some(reason.as_str())).then_some(Disagreement::ImpactDiffers { advisory, ignored: identifier })
        }
    };
    let status = (statement.status != Status::NotAffected).then_some(Disagreement::NotNotAffected {
        advisory,
        status: statement.status,
    });
    let justification = statement.justification.is_none().then_some(Disagreement::Unjustified { advisory });
    let undated = statement.timestamp.is_none().then_some(Disagreement::Undated { advisory });
    [ignore, status, justification, undated].into_iter().flatten().collect()
}

/// How the statements the aliases of `statement` name, among the first
/// statement of each name in `named`, disagree with it: an alias names no
/// statement, or its statement lists other identifiers, gives another
/// justification or names other crates.
fn alias_disagreements<'a>(statement: &'a Statement, named: &BTreeMap<&str, &'a Statement>) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    let identifiers = statement.vulnerability.identifiers();
    statement
        .vulnerability
        .aliases
        .iter()
        .filter_map(|alias| {
            let Some(aliased) = named.get(alias.as_str()) else {
                return Some(Disagreement::AliasWithoutStatement { advisory, alias });
            };
            if aliased.vulnerability.identifiers() != identifiers {
                Some(Disagreement::AliasesDiffer { advisory, alias })
            } else if aliased.justification != statement.justification || aliased.subcomponents() != statement.subcomponents() {
                Some(Disagreement::AliasRulesOtherwise { advisory, alias })
            } else {
                None
            }
        })
        .collect()
}

/// How the products of `statement` disagree with the broker `image`: a product
/// is no form of the image or names no crate inside it, or a form of the image
/// is named by no product.
fn product_disagreements<'a>(statement: &'a Statement, image: &'a BrokerImage) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    if statement.products.is_empty() {
        return vec![Disagreement::NoProduct { advisory }];
    }
    let forms: BTreeSet<ImageForm> = statement.products.iter().filter_map(|product| image.form_named_by(&product.id)).collect();
    let foreign = statement
        .products
        .iter()
        .filter(|product| image.form_named_by(&product.id).is_none())
        .map(|product| Disagreement::ProductNotImage {
            advisory,
            product: &product.id,
            oci: image.purl(ImageForm::Oci),
            docker: image.purl(ImageForm::Docker),
        });
    let missing = ImageForm::iter()
        .filter(|form| !forms.contains(form))
        .map(|form| Disagreement::ImageFormMissing {
            advisory,
            form: image.purl(form),
        });
    let empty = statement
        .products
        .iter()
        .filter(|product| product.subcomponents.is_empty())
        .map(|product| Disagreement::NoSubcomponent {
            advisory,
            product: &product.id,
        });
    foreign.chain(missing).chain(empty).collect()
}

/// How the crates `statement` names inside its products, each once, disagree
/// with the crates `locked` holds.
fn crate_disagreements<'a>(statement: &'a Statement, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    statement
        .subcomponents()
        .into_iter()
        .flat_map(|subcomponent| subcomponent_disagreements(statement, subcomponent, locked))
        .collect()
}

/// How the crate version `subcomponent` names disagrees with the crates
/// `locked` holds: it is no crate version, the lockfile does not hold it, or,
/// for a ruling that rests on what the linking crates call, a crate linking it
/// goes unnamed in the impact statement of `statement`.
fn subcomponent_disagreements<'a>(statement: &'a Statement, subcomponent: &'a str, locked: &'a LockedCrates) -> Vec<Disagreement<'a>> {
    let advisory = statement.vulnerability.name.as_str();
    let purl = PackageUrl::from_str(subcomponent).ok().filter(|purl| purl.ty() == CRATE_TYPE);
    let Some((name, version)) = purl.as_ref().and_then(|purl| Some((purl.name(), purl.version()?))) else {
        return vec![Disagreement::SubcomponentNotCrate { advisory, subcomponent }];
    };
    if !locked.holds(name, version) {
        return vec![Disagreement::NotLocked { advisory, subcomponent }];
    }
    if !statement.justification.is_some_and(Justification::rests_on_callers) {
        return Vec::new();
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
