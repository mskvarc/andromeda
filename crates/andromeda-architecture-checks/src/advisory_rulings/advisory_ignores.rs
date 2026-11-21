//! The advisories `deny.toml` ignores, read from its `advisories.ignore` list.
//!
//! cargo-deny takes an entry as an advisory id, as a table naming the id and
//! the reason, or as a table naming a crate. Only the first two ignore an
//! advisory.

use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Value};

/// Every advisory id `document` ignores, with the reason it records; `None`
/// for an id ignored without one.
pub(crate) fn advisory_ignores(document: &DocumentMut) -> BTreeMap<String, Option<String>> {
    document
        .get("advisories")
        .and_then(|advisories| advisories.get("ignore"))
        .and_then(Item::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Value::String(id) => Some((id.value().to_owned(), None)),
            Value::InlineTable(table) => table
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), table.get("reason").and_then(Value::as_str).map(str::to_owned))),
            Value::Integer(_) | Value::Float(_) | Value::Boolean(_) | Value::Datetime(_) | Value::Array(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::advisory_ignores::advisory_ignores;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use toml_edit::DocumentMut;

    fn ignores(configuration: &str) -> BTreeMap<String, Option<String>> {
        advisory_ignores(&configuration.parse::<DocumentMut>().unwrap())
    }

    #[test]
    fn an_ignore_table_gives_its_id_and_reason() {
        assert_eq!(
            ignores("[advisories]\nignore = [{ id = \"RUSTSEC-2026-0049\", reason = \"unreachable\" }]\n"),
            BTreeMap::from([("RUSTSEC-2026-0049".to_owned(), Some("unreachable".to_owned()))]),
        );
    }

    #[test]
    fn a_bare_id_is_ignored_without_a_reason() {
        assert_eq!(
            ignores("[advisories]\nignore = [\"RUSTSEC-2026-0049\"]\n"),
            BTreeMap::from([("RUSTSEC-2026-0049".to_owned(), None)])
        );
    }

    #[test]
    fn a_crate_ignore_names_no_advisory() {
        assert_eq!(
            ignores("[advisories]\nignore = [{ crate = \"rustls-webpki@0.102.8\", reason = \"unreachable\" }]\n"),
            BTreeMap::new()
        );
    }

    #[test]
    fn a_configuration_without_ignores_ignores_nothing() {
        assert_eq!(ignores("[advisories]\nyanked = \"deny\"\n"), BTreeMap::new());
        assert_eq!(ignores("[bans]\nwildcards = \"deny\"\n"), BTreeMap::new());
    }
}
