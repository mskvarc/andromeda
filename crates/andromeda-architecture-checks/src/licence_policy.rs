//! cargo-about accepts the licences cargo-deny allows.
//!
//! `deny.toml` decides which licences a dependency may come under, and
//! cargo-deny fails a dependency under any other. `about.toml` lists the
//! licences cargo-about may name a crate's licence as when it writes the
//! licence file the broker image ships, in the order it prefers them. The two
//! lists state one policy, so the gate holds them to the same licences, and
//! holds every licence `about.toml` grants a single crate to an exception
//! `deny.toml` makes for that crate. An exception for a crate the binary does
//! not link needs no entry in `about.toml`, where cargo-about warns about it.

use crate::workspace_file::read_toml_document;
use std::{collections::BTreeSet, path::Path};
use toml_edit::{DocumentMut, Item, Value};

/// The cargo-deny configuration, relative to the workspace root.
const DENY_CONFIGURATION: &str = "deny.toml";

/// The cargo-about configuration, relative to the workspace root.
const ABOUT_CONFIGURATION: &str = "about.toml";

/// Assert that `about.toml` accepts exactly the licences `deny.toml` allows,
/// and grants a single crate only a licence `deny.toml` excepts it for.
///
/// # Panics
///
/// Panics when the two configurations disagree, or when one of them cannot be
/// read or parsed.
pub fn assert_cargo_about_accepts_the_licences_cargo_deny_allows(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    let deny = read_toml_document(root, DENY_CONFIGURATION, &mut violations);
    let about = read_toml_document(root, ABOUT_CONFIGURATION, &mut violations);
    if let (Some(deny), Some(about)) = (deny, about) {
        violations.extend(disagreements(&deny, &about));
    }
    assert!(violations.is_empty(), "licence policies that disagree:\n{}", violations.join("\n"));
}

/// Every way the licences `about` accepts differ from the ones `deny` allows,
/// globally and for single crates.
fn disagreements(deny: &DocumentMut, about: &DocumentMut) -> Vec<String> {
    let licenses = deny.get("licenses");
    let allowed = licences(licenses.and_then(|licenses| licenses.get("allow")));
    let accepted = licences(about.get("accepted"));
    let unallowed = accepted
        .difference(&allowed)
        .map(|licence| format!("{ABOUT_CONFIGURATION} accepts {licence}, which {DENY_CONFIGURATION} does not allow"));
    let unaccepted = allowed
        .difference(&accepted)
        .map(|licence| format!("{DENY_CONFIGURATION} allows {licence}, which {ABOUT_CONFIGURATION} does not accept"));
    let unexcepted = crate_acceptances(about).flat_map(|(krate, granted)| {
        let excepted = exception(licenses, krate);
        granted
            .difference(&excepted)
            .map(|licence| format!("{ABOUT_CONFIGURATION} accepts {licence} for {krate}, for which {DENY_CONFIGURATION} makes no such exception"))
            .collect::<Vec<_>>()
    });
    unallowed.chain(unaccepted).chain(unexcepted).collect()
}

/// The licences an array of licence expressions names; empty when `item` is
/// absent or no array.
fn licences(item: Option<&Item>) -> BTreeSet<&str> {
    item.and_then(Item::as_array).into_iter().flatten().filter_map(Value::as_str).collect()
}

/// Every crate `about` grants licences of its own, with those licences. A
/// table of the configuration is a crate's when it lists `accepted`.
fn crate_acceptances(about: &DocumentMut) -> impl Iterator<Item = (&str, BTreeSet<&str>)> {
    about
        .iter()
        .filter_map(|(krate, item)| Some((krate, licences(Some(item.as_table_like()?.get("accepted")?)))))
}

/// The licences the `[[licenses.exceptions]]` of `deny.toml` allow `krate`;
/// empty when it makes no exception for the crate.
fn exception<'a>(licenses: Option<&'a Item>, krate: &str) -> BTreeSet<&'a str> {
    licenses
        .and_then(|licenses| licenses.get("exceptions"))
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten()
        .filter(|table| table.get("name").and_then(Item::as_str) == Some(krate))
        .flat_map(|table| licences(table.get("allow")))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::licence_policy::{assert_cargo_about_accepts_the_licences_cargo_deny_allows, disagreements};
    use pretty_assertions::assert_eq;
    use std::{env, fs, process};
    use toml_edit::DocumentMut;

    const DENY: &str = "[licenses]\nallow = [\"MIT\", \"Apache-2.0\", \"ISC\"]\n\n\
                        [[licenses.exceptions]]\nname = \"notify\"\nallow = [\"CC0-1.0\"]\n\n\
                        [[licenses.exceptions]]\nname = \"webpki-root-certs\"\nallow = [\"CDLA-Permissive-2.0\"]\n";

    fn findings(about: &str) -> Vec<String> {
        disagreements(&DENY.parse::<DocumentMut>().unwrap(), &about.parse::<DocumentMut>().unwrap())
    }

    #[test]
    fn the_same_licences_in_another_order_agree() {
        assert_eq!(findings("accepted = [\"ISC\", \"MIT\", \"Apache-2.0\"]\n"), Vec::<String>::new());
    }

    #[test]
    fn a_licence_only_about_toml_accepts_disagrees() {
        assert_eq!(
            findings("accepted = [\"MIT\", \"Apache-2.0\", \"ISC\", \"GPL-3.0\"]\n"),
            ["about.toml accepts GPL-3.0, which deny.toml does not allow"]
        );
    }

    #[test]
    fn a_licence_only_deny_toml_allows_disagrees() {
        assert_eq!(
            findings("accepted = [\"MIT\", \"Apache-2.0\"]\n"),
            ["deny.toml allows ISC, which about.toml does not accept"]
        );
    }

    #[test]
    fn a_crate_granted_a_licence_deny_toml_excepts_it_for_agrees() {
        assert_eq!(
            findings("accepted = [\"MIT\", \"Apache-2.0\", \"ISC\"]\nprivate = { ignore = true }\n\n[notify]\naccepted = [\"CC0-1.0\"]\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_crate_granted_a_licence_deny_toml_does_not_except_it_for_disagrees() {
        assert_eq!(
            findings(
                "accepted = [\"MIT\", \"Apache-2.0\", \"ISC\"]\n\n[notify]\naccepted = [\"CC0-1.0\", \"Unlicense\"]\n\n[ring]\naccepted = [\"OpenSSL\"]\n"
            ),
            [
                "about.toml accepts Unlicense for notify, for which deny.toml makes no such exception",
                "about.toml accepts OpenSSL for ring, for which deny.toml makes no such exception",
            ]
        );
    }

    #[test]
    #[should_panic(expected = "about.toml could not be read")]
    fn a_workspace_without_about_toml_fails() {
        let root = env::temp_dir().join(format!("andromeda-licence-policy-{}-absent", process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("deny.toml"), DENY).unwrap();
        assert_cargo_about_accepts_the_licences_cargo_deny_allows(&root.to_string_lossy());
    }
}
