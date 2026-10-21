//! Reading a manifest for a `chrono` dependency that enables its `serde`
//! feature, or a dependency on another date-time library.

use toml_edit::{DocumentMut, Item, TableLike};

/// Date-time libraries that would bring a second reader or writer of time
/// values beside the lexicon's.
const OTHER_DATE_TIME_LIBRARIES: [&str; 7] = ["chrono-tz", "dateparser", "iso8601", "jiff", "parse_datetime", "speedate", "time"];

/// The dependency tables a manifest may declare `chrono` in.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One finding per dependency table that enables `chrono/serde` and per other
/// date-time library it names, or the one finding that the manifest is not
/// TOML.
///
/// `chrono`'s `serde` feature gives `DateTime<Utc>` a reader of its own, which
/// admits numeric offsets and nanoseconds; with it off, a model member holding
/// an instant compiles only through the lexicon's `instant_serde` adapters.
pub(crate) fn date_time_library_findings(manifest: &str) -> Vec<String> {
    let document = match manifest.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return vec![format!("does not parse as TOML: {error}")],
    };
    let mut findings = Vec::new();
    for table in DEPENDENCY_TABLES {
        let tables = [document.get(table), document.get("workspace").and_then(|workspace| workspace.get(table))];
        for dependencies in tables.into_iter().flatten().filter_map(Item::as_table_like) {
            if enables_chrono_serde(dependencies) {
                findings.push(format!("[{table}] chrono enables its `serde` feature"));
            }
            for library in OTHER_DATE_TIME_LIBRARIES.iter().filter(|library| dependencies.contains_key(library)) {
                findings.push(format!("[{table}] depends on the date-time library `{library}`"));
            }
        }
    }
    findings
}

/// Whether a dependency table declares `chrono` with the `serde` feature.
fn enables_chrono_serde(dependencies: &dyn TableLike) -> bool {
    dependencies
        .get("chrono")
        .and_then(|chrono| chrono.get("features"))
        .and_then(Item::as_array)
        .is_some_and(|features| features.iter().any(|feature| feature.as_str() == Some("serde")))
}

#[cfg(test)]
mod tests {
    use crate::time_handling::manifest_reading::date_time_library_findings;

    #[test]
    fn chrono_with_the_serde_feature_is_a_finding() {
        let findings = date_time_library_findings("[dependencies]\nchrono = { version = \"0.4.45\", features = [\"serde\"] }\n");
        pretty_assertions::assert_eq!(findings, vec!["[dependencies] chrono enables its `serde` feature".to_owned()]);
    }

    #[test]
    fn chrono_in_a_dev_dependency_table_is_read_too() {
        let findings = date_time_library_findings("[dev-dependencies.chrono]\nversion = \"0.4.45\"\nfeatures = [\"std\", \"serde\"]\n");
        pretty_assertions::assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn another_date_time_library_is_a_finding() {
        let findings = date_time_library_findings("[dependencies]\njiff = \"0.2.38\"\ntime = { version = \"0.3.44\" }\n");
        pretty_assertions::assert_eq!(findings.len(), 2, "{findings:?}");
    }

    #[test]
    fn chrono_without_the_serde_feature_is_clean() {
        pretty_assertions::assert_eq!(
            date_time_library_findings("[dependencies]\nchrono = \"0.4.45\"\nserde = { version = \"1.0.229\", features = [\"derive\"] }\n"),
            Vec::<String>::new()
        );
        pretty_assertions::assert_eq!(
            date_time_library_findings("[dependencies]\nchrono = { version = \"0.4.45\", features = [\"clock\"] }\n"),
            Vec::<String>::new()
        );
    }
}
