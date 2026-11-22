//! A way the rulings stop agreeing with `deny.toml` or `Cargo.lock`.

use strum::Display;

/// A ruling or an advisory ignore that disagrees with `deny.toml`, with
/// `Cargo.lock` or with another ruling. Each names the advisory by the id the
/// ignore names. A disagreement is a finding the gate reports, never an error
/// a caller handles.
#[derive(Debug, PartialEq, Eq, Display)]
pub(crate) enum Disagreement<'a> {
    /// `deny.toml` ignores the advisory, and no ruling names it.
    #[strum(to_string = "deny.toml ignores {advisory}, but the rulings file has no ruling on it, so scanners report it against the broker image")]
    IgnoreWithoutRuling {
        /// The ignored advisory.
        advisory: &'a str,
    },
    /// A ruling names an advisory `deny.toml` does not ignore.
    #[strum(to_string = "the rulings file rules on {advisory}, but deny.toml does not ignore it")]
    RulingNotIgnored {
        /// The ruled advisory.
        advisory: &'a str,
    },
    /// `deny.toml` ignores the advisory by its bare id.
    #[strum(to_string = "deny.toml ignores {advisory} without a reason for the VEX impact statements to repeat")]
    IgnoredWithoutReason {
        /// The ignored advisory.
        advisory: &'a str,
    },
    /// Two rulings, or one ruling twice, name the same identifier, so two
    /// statements would be named by it.
    #[strum(to_string = "the rulings file names {identifier} more than once")]
    IdentifierRepeated {
        /// The identifier named more than once.
        identifier: &'a str,
    },
    /// `Cargo.lock` holds no such crate version.
    #[strum(to_string = "the ruling on {advisory} names {name}@{version}, but Cargo.lock holds no such crate version")]
    NotLocked {
        /// The ruling's advisory.
        advisory: &'a str,
        /// The crate name.
        name: &'a str,
        /// The crate version.
        version: &'a str,
    },
    /// A crate linking the ruling's crate version goes unnamed in the reason
    /// of a ruling that rests on what such crates call, so the ruling was not
    /// proven for it.
    #[strum(
        to_string = "the ruling on {advisory} is about {name} {version}, which {dependent} {dependent_version} links, but the reason deny.toml gives does not name {dependent} {dependent_version}; prove the ruling for that crate and name it"
    )]
    DependentUnnamed {
        /// The ruling's advisory.
        advisory: &'a str,
        /// The crate name.
        name: &'a str,
        /// The crate version.
        version: &'a str,
        /// The name of the crate that links it.
        dependent: &'a str,
        /// The locked version of that crate.
        dependent_version: &'a str,
    },
    /// The reason relies on a crate at a version or later, and the lockfile
    /// holds no version of it that high.
    #[strum(to_string = "the reason deny.toml gives for {advisory} relies on {name} {floor} or later, but Cargo.lock holds no {name} at or above {floor}")]
    FloorNotLocked {
        /// The ruling's advisory.
        advisory: &'a str,
        /// The crate the reason names.
        name: &'a str,
        /// The oldest version it relies on, as the reason spells it.
        floor: &'a str,
    },
    /// The lockfile holds a version of a crate below the one the reason relies
    /// on, and the ruling is not about it. The advisory's ignore covers every
    /// version, so cargo-deny reports nothing for it either.
    #[strum(
        to_string = "Cargo.lock holds {name} {version}, which the ruling on {advisory} is not about, though its reason relies on {name} {floor} or later and the ignore of the advisory covers every version"
    )]
    BelowFloor {
        /// The ruling's advisory.
        advisory: &'a str,
        /// The crate the reason names.
        name: &'a str,
        /// The locked version below the floor.
        version: &'a str,
        /// The oldest version the reason relies on, as it spells it.
        floor: &'a str,
    },
}
