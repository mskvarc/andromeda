//! A way the VEX document can stop repeating the rulings it publishes.

use crate::advisory_rulings::vex_document::Status;
use strum::Display;

/// A VEX statement or an advisory ignore that disagrees with `deny.toml`, with
/// `Cargo.lock`, with the statements of the same advisory, or with the forms a
/// scanner matches the image's statements by. Each names the advisory as the
/// statement or the ignore spells it. A disagreement is a finding the gate
/// reports, never an error a caller handles.
#[derive(Debug, PartialEq, Eq, Display)]
pub(crate) enum Disagreement<'a> {
    /// `deny.toml` ignores the advisory, and no statement names it.
    #[strum(to_string = "deny.toml ignores {advisory}, but no VEX statement names it, so scanners report it against the broker image")]
    IgnoreWithoutStatement {
        /// The ignored advisory.
        advisory: &'a str,
    },
    /// No ignore in `deny.toml` names the advisory or any of its aliases.
    #[strum(to_string = "{advisory} has a VEX statement, but deny.toml ignores neither it nor any of its aliases")]
    NotIgnored {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// `deny.toml` ignores the advisory by its bare id.
    #[strum(to_string = "deny.toml ignores {ignored} without a reason for the VEX impact statement of {advisory} to repeat")]
    IgnoredWithoutReason {
        /// The statement's advisory.
        advisory: &'a str,
        /// The identifier of the advisory `deny.toml` ignores.
        ignored: &'a str,
    },
    /// The impact statement differs from the reason `deny.toml` records.
    #[strum(to_string = "the VEX impact statement of {advisory} differs from the reason deny.toml ignores {ignored} for")]
    ImpactDiffers {
        /// The statement's advisory.
        advisory: &'a str,
        /// The identifier of the advisory `deny.toml` ignores.
        ignored: &'a str,
    },
    /// An earlier statement already rules on the advisory.
    #[strum(to_string = "{advisory} has more than one VEX statement")]
    Repeated {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// An alias names no statement of its own, so a scanner that reports the
    /// advisory under the alias and matches statements by name skips it.
    #[strum(
        to_string = "the VEX statement of {advisory} lists the alias {alias}, but no statement is named {alias}, so a scanner reporting the advisory as {alias} does not apply it"
    )]
    AliasWithoutStatement {
        /// The statement's advisory.
        advisory: &'a str,
        /// The alias no statement is named by.
        alias: &'a str,
    },
    /// The statement an alias names lists other identifiers.
    #[strum(to_string = "the VEX statements of {advisory} and of its alias {alias} list different identifiers")]
    AliasesDiffer {
        /// The statement's advisory.
        advisory: &'a str,
        /// The alias whose statement lists other identifiers.
        alias: &'a str,
    },
    /// The statement an alias names gives another justification or names other
    /// crates.
    #[strum(to_string = "the VEX statement of {advisory} gives another justification or names other crates than the statement of its alias {alias}")]
    AliasRulesOtherwise {
        /// The statement's advisory.
        advisory: &'a str,
        /// The alias whose statement rules otherwise.
        alias: &'a str,
    },
    /// The statement's status is not the `not_affected` an ignore records.
    #[strum(to_string = "the VEX statement of {advisory} is {status}, but an advisory deny.toml ignores is not_affected")]
    NotNotAffected {
        /// The statement's advisory.
        advisory: &'a str,
        /// The status the statement gives.
        status: Status,
    },
    /// The statement gives no justification.
    #[strum(to_string = "the VEX statement of {advisory} gives no justification")]
    Unjustified {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// The statement carries no timestamp, so it takes the document's, which
    /// moves whenever any statement changes.
    #[strum(to_string = "the VEX statement of {advisory} has no timestamp of its own")]
    Undated {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// The statement names no product.
    #[strum(to_string = "the VEX statement of {advisory} names no product")]
    NoProduct {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// A product is the broker image in neither form a scanner matches every
    /// digest of it by.
    #[strum(to_string = "the VEX statement of {advisory} names {product}, but scanners match every digest of the broker image only as {oci} or {docker}")]
    ProductNotImage {
        /// The statement's advisory.
        advisory: &'a str,
        /// The product's package URL as the statement spells it.
        product: &'a str,
        /// The `oci` package URL naming every digest of the broker image.
        oci: &'a str,
        /// The `docker` package URL naming every digest of the broker image.
        docker: &'a str,
    },
    /// No product names the broker image in one of the forms scanners match it
    /// by, so a scanner using that form skips the statement.
    #[strum(to_string = "the VEX statement of {advisory} has no product {form}, so a scanner naming the broker image so does not apply it")]
    ImageFormMissing {
        /// The statement's advisory.
        advisory: &'a str,
        /// The package URL naming every digest of the broker image in the
        /// missing form.
        form: &'a str,
    },
    /// A product names no crate inside it.
    #[strum(to_string = "the VEX statement of {advisory} names no crate inside {product}")]
    NoSubcomponent {
        /// The statement's advisory.
        advisory: &'a str,
        /// The product's package URL as the statement spells it.
        product: &'a str,
    },
    /// A subcomponent is not a crate at one version.
    #[strum(to_string = "the VEX statement of {advisory} names {subcomponent}, which is no pkg:cargo package URL with a version")]
    SubcomponentNotCrate {
        /// The statement's advisory.
        advisory: &'a str,
        /// The subcomponent's package URL as the statement spells it.
        subcomponent: &'a str,
    },
    /// A crate linking the subcomponent's crate version goes unnamed in the
    /// impact statement of a ruling that rests on what such crates call, so
    /// the ruling was not proven for it.
    #[strum(
        to_string = "the VEX statement of {advisory} names {subcomponent}, which {dependent} {version} links, but its impact statement does not name {dependent} {version}; prove the ruling for that crate and name it"
    )]
    DependentUnnamed {
        /// The statement's advisory.
        advisory: &'a str,
        /// The subcomponent's package URL as the statement spells it.
        subcomponent: &'a str,
        /// The name of the crate that links the subcomponent's crate version.
        dependent: &'a str,
        /// The locked version of that crate.
        version: &'a str,
    },
    /// `Cargo.lock` holds no such crate version.
    #[strum(to_string = "the VEX statement of {advisory} names {subcomponent}, but Cargo.lock holds no such crate version")]
    NotLocked {
        /// The statement's advisory.
        advisory: &'a str,
        /// The subcomponent's package URL as the statement spells it.
        subcomponent: &'a str,
    },
}
