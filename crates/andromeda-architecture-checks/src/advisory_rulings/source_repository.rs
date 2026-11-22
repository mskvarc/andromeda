//! The GitHub repository the root manifest's `[workspace.package]` names. Its
//! owner publishes the broker image, and its default branch holds the VEX
//! document the document's own identifier points at.

use toml_edit::DocumentMut;
use url::Url;

/// The host of the source repository.
const SOURCE_HOST: &str = "github.com";

/// The branch whose files the document identifier points at.
const DEFAULT_BRANCH: &str = "main";

/// The workspace's GitHub repository.
#[derive(Debug)]
pub(crate) struct SourceRepository {
    /// The repository as the manifest names it.
    url: Url,
    /// The repository's owner, in lowercase, as a container registry spells it.
    owner: String,
}

impl SourceRepository {
    /// The repository the `[workspace.package]` of `manifest` names, or `None`
    /// when it names no GitHub repository.
    pub(crate) fn from_manifest(manifest: &DocumentMut) -> Option<SourceRepository> {
        let repository = manifest.get("workspace")?.get("package")?.get("repository")?.as_str()?;
        let url = Url::parse(repository).ok().filter(|url| url.host_str() == Some(SOURCE_HOST))?;
        let owner = url.path_segments()?.next().filter(|owner| !owner.is_empty())?.to_lowercase();
        Some(SourceRepository { url, owner })
    }

    /// The repository's owner, in lowercase.
    pub(crate) fn owner(&self) -> &str {
        &self.owner
    }

    /// The page of the file at the workspace-relative `path` on the default
    /// branch; `None` when the repository URL takes no path.
    pub(crate) fn file_on_default_branch(&self, path: &str) -> Option<Url> {
        let mut url = self.url.clone();
        url.path_segments_mut()
            .ok()?
            .pop_if_empty()
            .extend(["blob", DEFAULT_BRANCH])
            .extend(path.split('/'));
        Some(url)
    }
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::source_repository::SourceRepository;
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    fn repository(manifest: &str) -> Option<SourceRepository> {
        SourceRepository::from_manifest(&manifest.parse::<DocumentMut>().unwrap())
    }

    #[test]
    fn the_owner_of_a_github_repository_is_read_in_lowercase() {
        let repository = repository("[workspace.package]\nrepository = \"https://github.com/Example/andromeda\"\n").unwrap();
        assert_eq!(repository.owner(), "example");
    }

    #[test]
    fn a_file_is_found_on_the_default_branch_with_or_without_a_trailing_slash() {
        for manifest in [
            "[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n",
            "[workspace.package]\nrepository = \"https://github.com/example/andromeda/\"\n",
        ] {
            assert_eq!(
                repository(manifest)
                    .unwrap()
                    .file_on_default_branch(".vex/andromeda.openvex.json")
                    .unwrap()
                    .as_str(),
                "https://github.com/example/andromeda/blob/main/.vex/andromeda.openvex.json",
                "{manifest}"
            );
        }
    }

    #[test]
    fn a_manifest_without_a_github_repository_names_none() {
        for manifest in [
            "[workspace.package]\nrepository = \"https://gitlab.com/example/andromeda\"\n",
            "[workspace.package]\nrepository = \"https://github.com/\"\n",
            "[workspace.package]\nedition = \"2024\"\n",
            "[package]\nrepository = \"https://github.com/example/andromeda\"\n",
        ] {
            assert!(repository(manifest).is_none(), "{manifest}");
        }
    }
}
