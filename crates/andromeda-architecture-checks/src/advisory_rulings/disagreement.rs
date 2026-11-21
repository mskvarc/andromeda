//! A way a VEX statement can stop repeating the ruling it publishes.

use crate::advisory_rulings::vex_document::Status;
use strum::Display;

/// A VEX statement that disagrees with `deny.toml`, with `Cargo.lock`, or with
/// the form a scanner matches the image's statements by. Each names the
/// advisory of the statement. A disagreement is a finding the gate reports,
/// never an error a caller handles.
#[derive(Debug, PartialEq, Eq, Display)]
pub(crate) enum Disagreement<'a> {
    /// No ignore in `deny.toml` names the advisory.
    #[strum(to_string = "{advisory} has a VEX statement, but deny.toml ignores no such advisory")]
    NotIgnored {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// `deny.toml` ignores the advisory by its bare id.
    #[strum(to_string = "deny.toml ignores {advisory} without a reason for its VEX impact statement to repeat")]
    IgnoredWithoutReason {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// The impact statement differs from the reason `deny.toml` records.
    #[strum(to_string = "the VEX impact statement of {advisory} differs from the reason deny.toml ignores it for")]
    ImpactDiffers {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// An earlier statement already rules on the advisory.
    #[strum(to_string = "{advisory} has more than one VEX statement")]
    Repeated {
        /// The statement's advisory.
        advisory: &'a str,
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
    /// The statement names no product.
    #[strum(to_string = "the VEX statement of {advisory} names no product")]
    NoProduct {
        /// The statement's advisory.
        advisory: &'a str,
    },
    /// A product is not the broker image named the way a scanner matches every
    /// digest of it.
    #[strum(to_string = "the VEX statement of {advisory} names {product}, but a scanner matches every digest of the broker image only as {image}")]
    ProductNotImage {
        /// The statement's advisory.
        advisory: &'a str,
        /// The product's package URL as the statement spells it.
        product: &'a str,
        /// The package URL naming every digest of the broker image.
        image: &'a str,
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
    /// impact statement, so the ruling was not proven for what it calls.
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
