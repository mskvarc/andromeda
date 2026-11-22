//! The rulings file: one table per advisory `deny.toml` ignores, named by the
//! id the ignore names, giving the advisory's other identifiers, the OpenVEX
//! justification and the crate versions the ruling is about.
//!
//! The reason of a ruling is the ignore's own, so the file never repeats it,
//! and the VEX document is generated from both.

use crate::advisory_rulings::vex_document::Justification;
use semver::Version;
use std::{collections::BTreeMap, str::FromStr};
use strum::ParseError;
use thiserror::Error;
use toml_edit::{DocumentMut, Item, TableLike, Value};

/// The keys a ruling's table may hold.
const RULING_KEYS: [&str; 3] = ["aliases", "crates", "justification"];

/// What the rulings file records about one advisory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ruling {
    /// Every identifier of the advisory, the one `deny.toml` ignores first and
    /// its aliases after it in the order the file lists them.
    pub(crate) identifiers: Vec<String>,
    /// Why the broker image is not affected.
    pub(crate) justification: Justification,
    /// The crate versions the ruling is about; never empty.
    pub(crate) crates: Vec<CrateVersion>,
}

/// One crate at one version, spelt `<name>@<version>` in the file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CrateVersion {
    /// The crate name.
    pub(crate) name: String,
    /// The version, a semantic version.
    pub(crate) version: String,
}

/// Why a ruling's table cannot be read.
#[derive(Debug, Error)]
enum RulingError {
    /// The entry is no table.
    #[error("is no table")]
    NotATable,
    /// The table holds a key a ruling has no use for.
    #[error("holds a key other than aliases, crates and justification")]
    UnknownKey,
    /// The aliases are not a list of identifiers.
    #[error("lists aliases that are not all strings")]
    AliasesNotStrings,
    /// The table gives no justification as a string.
    #[error("gives no justification")]
    NoJustification,
    /// The justification is no OpenVEX token.
    #[error("gives a justification OpenVEX does not define")]
    UnknownJustification(#[from] ParseError),
    /// The table names no crate version.
    #[error("names no crate as <name>@<version>")]
    NoCrate,
    /// A crate entry is not a crate at a semantic version.
    #[error("names crate {position} other than as <name>@<version> at a semantic version")]
    CrateNotNameAtVersion {
        /// The entry's position in the list, counted from 1.
        position: usize,
    },
}

/// Every ruling `document` records, by the advisory id it is named by. A
/// ruling that cannot be read is left out, with the reason recorded in
/// `violations` under the file's name `file`.
pub(crate) fn read_rulings(document: &DocumentMut, file: &str, violations: &mut Vec<String>) -> BTreeMap<String, Ruling> {
    document
        .iter()
        .filter_map(|(advisory, item)| match read_ruling(advisory, item) {
            Ok(ruling) => Some((advisory.to_owned(), ruling)),
            Err(error) => {
                violations.push(format!("the ruling {advisory} in {file} {error}"));
                None
            }
        })
        .collect()
}

/// The ruling on `advisory` that `item` records.
fn read_ruling(advisory: &str, item: &Item) -> Result<Ruling, RulingError> {
    let table = item.as_table_like().ok_or(RulingError::NotATable)?;
    if table.iter().any(|(key, _)| !RULING_KEYS.contains(&key)) {
        return Err(RulingError::UnknownKey);
    }
    let aliases = strings(table, "aliases").ok_or(RulingError::AliasesNotStrings)?;
    let justification = Justification::from_str(table.get("justification").and_then(Item::as_str).ok_or(RulingError::NoJustification)?)?;
    let crates = strings(table, "crates")
        .ok_or(RulingError::NoCrate)?
        .into_iter()
        .zip(1..)
        .map(|(spelt, position)| CrateVersion::parse(spelt).ok_or(RulingError::CrateNotNameAtVersion { position }))
        .collect::<Result<Vec<_>, _>>()?;
    if crates.is_empty() {
        return Err(RulingError::NoCrate);
    }
    Ok(Ruling {
        identifiers: [advisory].into_iter().chain(aliases).map(str::to_owned).collect(),
        justification,
        crates,
    })
}

/// The strings the list at `key` holds; empty when the table has no `key`, and
/// `None` when it holds something other than a list of strings.
fn strings<'a>(table: &'a dyn TableLike, key: &str) -> Option<Vec<&'a str>> {
    let Some(item) = table.get(key) else {
        return Some(Vec::new());
    };
    item.as_array()?.iter().map(Value::as_str).collect()
}

impl CrateVersion {
    /// The crate version `spelt` names as `<name>@<version>`, or `None` when it
    /// names none.
    fn parse(spelt: &str) -> Option<CrateVersion> {
        let (name, version) = spelt.split_once('@')?;
        (!name.is_empty() && Version::parse(version).is_ok()).then(|| CrateVersion {
            name: name.to_owned(),
            version: version.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::{
        rulings_file::{CrateVersion, Ruling, read_rulings},
        vex_document::Justification,
    };
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use toml_edit::DocumentMut;

    fn read(file: &str) -> (BTreeMap<String, Ruling>, Vec<String>) {
        let mut violations = Vec::new();
        let rulings = read_rulings(&file.parse::<DocumentMut>().unwrap(), "rulings.toml", &mut violations);
        (rulings, violations)
    }

    #[test]
    fn a_ruling_lists_its_id_first_then_its_aliases() {
        let (rulings, violations) = read(
            "[RUSTSEC-2026-0049]\naliases = [\"CVE-2026-93602\", \"GHSA-pwjx-qhcg-rvj4\"]\njustification = \"vulnerable_code_not_in_execute_path\"\ncrates = [\"rustls-webpki@0.102.8\"]\n",
        );
        assert_eq!(violations, Vec::<String>::new());
        assert_eq!(
            rulings,
            BTreeMap::from([(
                "RUSTSEC-2026-0049".to_owned(),
                Ruling {
                    identifiers: vec!["RUSTSEC-2026-0049".to_owned(), "CVE-2026-93602".to_owned(), "GHSA-pwjx-qhcg-rvj4".to_owned()],
                    justification: Justification::VulnerableCodeNotInExecutePath,
                    crates: vec![CrateVersion {
                        name: "rustls-webpki".to_owned(),
                        version: "0.102.8".to_owned(),
                    }],
                }
            )])
        );
    }

    #[test]
    fn a_ruling_without_aliases_is_known_by_its_id_alone() {
        let (rulings, _) = read("[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\n");
        assert_eq!(rulings["RUSTSEC-2024-0370"].identifiers, ["RUSTSEC-2024-0370"]);
    }

    #[test]
    fn a_ruling_that_cannot_be_read_is_left_out_with_its_reason() {
        for (file, reason) in [
            ("RUSTSEC-2024-0370 = \"component_not_present\"\n", "is no table"),
            (
                "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\nreason = \"unreachable\"\n",
                "holds a key other than",
            ),
            (
                "[RUSTSEC-2024-0370]\naliases = [1]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\n",
                "lists aliases",
            ),
            ("[RUSTSEC-2024-0370]\ncrates = [\"proc-macro-error@1.0.4\"]\n", "gives no justification"),
            (
                "[RUSTSEC-2024-0370]\njustification = \"unreachable\"\ncrates = [\"proc-macro-error@1.0.4\"]\n",
                "justification OpenVEX does not define",
            ),
            ("[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\n", "names no crate"),
            (
                "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = []\n",
                "names no crate",
            ),
            (
                "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error 1.0.4\"]\n",
                "names crate 1 other than",
            ),
            (
                "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0\"]\n",
                "names crate 1 other than",
            ),
            (
                "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\", \"@1.0.4\"]\n",
                "names crate 2 other than",
            ),
        ] {
            let (rulings, violations) = read(file);
            assert!(rulings.is_empty(), "{file}");
            let [violation] = violations.as_slice() else {
                panic!("one violation for {file}: {violations:?}");
            };
            assert!(violation.starts_with("the ruling RUSTSEC-2024-0370 in rulings.toml "), "{violation}");
            assert!(violation.contains(reason), "{violation} does not say {reason}");
        }
    }
}
