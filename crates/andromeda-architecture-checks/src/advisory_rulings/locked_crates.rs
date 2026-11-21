//! The crate versions `Cargo.lock` holds, and the packages that depend on
//! each, read from its `package` entries.
//!
//! A lockfile entry lists a dependency by name alone when it holds one version
//! of that crate, by name and version when it holds several, and adds the
//! source in parentheses when two sources hold the same version.

use toml_edit::{DocumentMut, Item, Value};

/// The packages a lockfile holds.
#[derive(Debug, Default)]
pub(crate) struct LockedCrates {
    /// Every locked package, in lockfile order.
    packages: Vec<LockedPackage>,
}

/// One package of the lockfile.
#[derive(Debug)]
pub(crate) struct LockedPackage {
    /// The crate name.
    pub(crate) name: String,
    /// The locked version.
    pub(crate) version: String,
    /// Each dependency as the lockfile spells it.
    dependencies: Vec<String>,
}

impl LockedCrates {
    /// The packages `document` locks; a package entry without a name or a
    /// version is left out.
    pub(crate) fn from_document(document: &DocumentMut) -> LockedCrates {
        let packages = document
            .get("package")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
            .filter_map(|package| {
                let dependencies = package
                    .get("dependencies")
                    .and_then(Item::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(LockedPackage {
                    name: package.get("name")?.as_str()?.to_owned(),
                    version: package.get("version")?.as_str()?.to_owned(),
                    dependencies,
                })
            })
            .collect();
        LockedCrates { packages }
    }

    /// Whether the lockfile holds `name` at `version`.
    pub(crate) fn holds(&self, name: &str, version: &str) -> bool {
        self.packages.iter().any(|package| package.name == name && package.version == version)
    }

    /// Every package that depends on `name` at `version`, which the lockfile
    /// holds.
    pub(crate) fn dependents<'a>(&'a self, name: &str, version: &str) -> impl Iterator<Item = &'a LockedPackage> {
        self.packages
            .iter()
            .filter(move |package| package.dependencies.iter().any(|dependency| names(dependency, name, version)))
    }
}

/// Whether the dependency entry `dependency` names `name` at `version`. An
/// entry without a version names the one version the lockfile holds.
fn names(dependency: &str, name: &str, version: &str) -> bool {
    let mut words = dependency.split_whitespace();
    words.next() == Some(name) && words.next().is_none_or(|named| named == version)
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::locked_crates::LockedCrates;
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    const LOCKFILE: &str = "version = 4\n\n\
                            [[package]]\nname = \"rumqttc\"\nversion = \"0.25.1\"\ndependencies = [\n \"rustls-pemfile\",\n \"rustls-webpki 0.102.8\",\n]\n\n\
                            [[package]]\nname = \"rustls\"\nversion = \"0.23.46\"\ndependencies = [\n \"rustls-webpki 0.103.17 (registry+https://github.com/rust-lang/crates.io-index)\",\n]\n\n\
                            [[package]]\nname = \"rustls-pemfile\"\nversion = \"2.2.0\"\n\n\
                            [[package]]\nname = \"rustls-webpki\"\nversion = \"0.102.8\"\n\n\
                            [[package]]\nname = \"rustls-webpki\"\nversion = \"0.103.17\"\n";

    fn locked() -> LockedCrates {
        LockedCrates::from_document(&LOCKFILE.parse::<DocumentMut>().unwrap())
    }

    fn dependents(name: &str, version: &str) -> Vec<(String, String)> {
        locked()
            .dependents(name, version)
            .map(|package| (package.name.clone(), package.version.clone()))
            .collect()
    }

    #[test]
    fn every_locked_version_of_a_crate_is_held() {
        let locked = locked();
        assert!(locked.holds("rustls-webpki", "0.102.8"));
        assert!(locked.holds("rustls-webpki", "0.103.17"));
        assert!(!locked.holds("rustls-webpki", "0.102.9"));
        assert!(!locked.holds("webpki", "0.102.8"));
    }

    #[test]
    fn a_dependent_is_found_by_the_version_its_entry_names() {
        assert_eq!(dependents("rustls-webpki", "0.102.8"), [("rumqttc".to_owned(), "0.25.1".to_owned())]);
    }

    #[test]
    fn a_dependent_is_found_through_an_entry_that_also_names_the_source() {
        assert_eq!(dependents("rustls-webpki", "0.103.17"), [("rustls".to_owned(), "0.23.46".to_owned())]);
    }

    #[test]
    fn a_dependent_is_found_through_an_entry_naming_the_crate_alone() {
        assert_eq!(dependents("rustls-pemfile", "2.2.0"), [("rumqttc".to_owned(), "0.25.1".to_owned())]);
    }

    #[test]
    fn a_lockfile_without_packages_holds_nothing() {
        let locked = LockedCrates::from_document(&"version = 4\n".parse::<DocumentMut>().unwrap());
        assert!(!locked.holds("rustls-webpki", "0.102.8"));
        assert_eq!(locked.dependents("rustls-webpki", "0.102.8").count(), 0);
    }
}
