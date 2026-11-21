//! The broker image as a scanner names it, the product every VEX statement
//! has to name for a scanner to apply it.
//!
//! The release publishes the image to the GitHub container registry under the
//! owner of the source repository, as `ghcr.io/<owner>/andromeda`. A scanner
//! names a pulled image by the last segment of its repository, with the whole
//! repository as the `repository_url` qualifier and the digest as the version,
//! and a statement applies when its product names the same image with a subset
//! of those qualifiers. A product without a version covers every digest; one
//! naming another repository or image matches nothing.

use packageurl::PackageUrl;
use std::str::FromStr;
use toml_edit::DocumentMut;
use url::Url;

/// The registry the release publishes the broker image to.
const IMAGE_REGISTRY: &str = "ghcr.io";

/// The host of the source repository, whose owner the image is published under.
const SOURCE_HOST: &str = "github.com";

/// The name the broker image is published under, which is also the name of its
/// package URL.
const IMAGE_NAME: &str = "andromeda";

/// The package URL type of an OCI image.
const IMAGE_TYPE: &str = "oci";

/// The package URL qualifier naming the repository an image is pulled from.
const REPOSITORY_QUALIFIER: &str = "repository_url";

/// The broker image's repository, and the package URL that names every digest
/// of it.
#[derive(Debug)]
pub(crate) struct BrokerImage {
    /// The repository the image is pulled from, `ghcr.io/<owner>/andromeda`.
    repository: String,
    /// The package URL naming every digest of the image.
    purl: String,
}

impl BrokerImage {
    /// The image published under the owner of the `[workspace.package]`
    /// repository `manifest` declares, or `None` when it declares no GitHub
    /// repository.
    pub(crate) fn from_manifest(manifest: &DocumentMut) -> Option<BrokerImage> {
        let repository = manifest.get("workspace")?.get("package")?.get("repository")?.as_str()?;
        let url = Url::parse(repository).ok().filter(|url| url.host_str() == Some(SOURCE_HOST))?;
        let owner = url.path_segments()?.next().filter(|owner| !owner.is_empty())?.to_lowercase();
        let repository = format!("{IMAGE_REGISTRY}/{owner}/{IMAGE_NAME}");
        let purl = format!("pkg:{IMAGE_TYPE}/{IMAGE_NAME}?{REPOSITORY_QUALIFIER}={repository}");
        Some(BrokerImage { repository, purl })
    }

    /// The package URL naming every digest of the image.
    pub(crate) fn purl(&self) -> &str {
        &self.purl
    }

    /// Whether the package URL `id` names every digest of the image, as an
    /// `oci` package URL of its name and repository with no version. Further
    /// qualifiers, such as an architecture, narrow the product and are allowed.
    pub(crate) fn is_named_by(&self, id: &str) -> bool {
        PackageUrl::from_str(id).is_ok_and(|purl| {
            purl.ty() == IMAGE_TYPE
                && purl.namespace().is_none()
                && purl.name() == IMAGE_NAME
                && purl.version().is_none()
                && purl
                    .qualifiers()
                    .get(REPOSITORY_QUALIFIER)
                    .is_some_and(|repository| *repository == self.repository)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::broker_image::BrokerImage;
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    fn image(manifest: &str) -> Option<BrokerImage> {
        BrokerImage::from_manifest(&manifest.parse::<DocumentMut>().unwrap())
    }

    #[test]
    fn the_image_is_published_under_the_owner_of_the_source_repository() {
        let image = image("[workspace.package]\nrepository = \"https://github.com/Example/andromeda\"\n").unwrap();
        assert_eq!(image.purl(), "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda");
    }

    #[test]
    fn a_manifest_without_a_github_repository_names_no_image() {
        for manifest in [
            "[workspace.package]\nrepository = \"https://gitlab.com/example/andromeda\"\n",
            "[workspace.package]\nrepository = \"https://github.com/\"\n",
            "[workspace.package]\nedition = \"2024\"\n",
            "[package]\nrepository = \"https://github.com/example/andromeda\"\n",
        ] {
            assert!(image(manifest).is_none(), "{manifest}");
        }
    }

    #[test]
    fn only_a_versionless_package_url_of_the_image_and_its_repository_names_it() {
        let image = image("[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n").unwrap();
        for (id, names) in [
            ("pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda", true),
            ("pkg:oci/andromeda?repository_url=ghcr.io%2Fexample%2Fandromeda", true),
            ("pkg:oci/andromeda?arch=amd64&repository_url=ghcr.io/example/andromeda", true),
            ("pkg:oci/andromedaa?repository_url=ghcr.io/example/andromeda", false),
            ("pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda", false),
            ("pkg:oci/andromeda?repository_url=docker.io/example/andromeda", false),
            ("pkg:oci/andromeda", false),
            ("pkg:oci/andromeda@sha256:0123?repository_url=ghcr.io/example/andromeda", false),
            ("pkg:docker/andromeda?repository_url=ghcr.io/example/andromeda", false),
            ("ghcr.io/example/andromeda", false),
        ] {
            assert_eq!(image.is_named_by(id), names, "{id}");
        }
    }
}
