//! The broker image as scanners name it, the products every VEX statement
//! has to name for every scanner to apply it.
//!
//! The release publishes the image to the GitHub container registry under the
//! owner of the source repository, as `ghcr.io/<owner>/andromeda`. Scanners
//! name a pulled image in one of two package URL forms. The `oci` form names
//! it by the last segment of its repository, with the whole repository as the
//! `repository_url` qualifier. The `docker` form names it by its owner and
//! name, with the registry as the `repository_url` qualifier. Either carries
//! the digest as the version, and a scanner applies a statement when one of its
//! products names the same image in the scanner's form with a subset of those
//! qualifiers. A product with no version and no qualifier but `repository_url`
//! covers every digest. A further qualifier, such as `arch`, narrows it to the
//! digests a scanner names with that qualifier, and a product naming another
//! repository or image matches nothing.

use packageurl::PackageUrl;
use std::str::FromStr;
use strum::EnumIter;
use toml_edit::DocumentMut;
use url::Url;

/// The registry the release publishes the broker image to.
const IMAGE_REGISTRY: &str = "ghcr.io";

/// The host of the source repository, whose owner the image is published under.
const SOURCE_HOST: &str = "github.com";

/// The name the broker image is published under, which is also the name of its
/// package URLs.
const IMAGE_NAME: &str = "andromeda";

/// The package URL type of an OCI image.
const OCI_TYPE: &str = "oci";

/// The package URL type of a Docker image.
const DOCKER_TYPE: &str = "docker";

/// The package URL qualifier naming where an image is pulled from.
const REPOSITORY_QUALIFIER: &str = "repository_url";

/// A package URL form scanners name the broker image in. A scanner matches
/// statements by one form alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, EnumIter)]
pub(crate) enum ImageForm {
    /// `pkg:oci/andromeda?repository_url=ghcr.io/<owner>/andromeda`.
    Oci,
    /// `pkg:docker/<owner>/andromeda?repository_url=ghcr.io`.
    Docker,
}

/// The broker image's owner and repository, and the package URLs that name
/// every digest of it.
#[derive(Debug)]
pub(crate) struct BrokerImage {
    /// The owner the image is published under, in lowercase.
    owner: String,
    /// The repository the image is pulled from, `ghcr.io/<owner>/andromeda`.
    repository: String,
    /// The `oci` package URL naming every digest of the image.
    oci: String,
    /// The `docker` package URL naming every digest of the image.
    docker: String,
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
        let oci = format!("pkg:{OCI_TYPE}/{IMAGE_NAME}?{REPOSITORY_QUALIFIER}={repository}");
        let docker = format!("pkg:{DOCKER_TYPE}/{owner}/{IMAGE_NAME}?{REPOSITORY_QUALIFIER}={IMAGE_REGISTRY}");
        Some(BrokerImage {
            owner,
            repository,
            oci,
            docker,
        })
    }

    /// The package URL in `form` naming every digest of the image.
    pub(crate) fn purl(&self, form: ImageForm) -> &str {
        match form {
            ImageForm::Oci => &self.oci,
            ImageForm::Docker => &self.docker,
        }
    }

    /// The form in which the package URL `id` names every digest of the image,
    /// with no version and no qualifier but `repository_url`: an `oci` package
    /// URL of its name and repository, or a `docker` one of its owner and name
    /// in its registry. `None` when `id` names neither, or narrows the image to
    /// some of its digests with a further qualifier, such as an architecture.
    pub(crate) fn form_named_by(&self, id: &str) -> Option<ImageForm> {
        let purl = PackageUrl::from_str(id).ok().filter(|purl| {
            purl.name() == IMAGE_NAME && purl.version().is_none() && purl.qualifiers().keys().all(|qualifier| *qualifier == REPOSITORY_QUALIFIER)
        })?;
        let pulled_from = purl.qualifiers().get(REPOSITORY_QUALIFIER)?;
        if purl.ty() == OCI_TYPE && purl.namespace().is_none() && *pulled_from == self.repository {
            Some(ImageForm::Oci)
        } else if purl.ty() == DOCKER_TYPE && purl.namespace() == Some(self.owner.as_str()) && *pulled_from == IMAGE_REGISTRY {
            Some(ImageForm::Docker)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::broker_image::{BrokerImage, ImageForm};
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    fn image(manifest: &str) -> Option<BrokerImage> {
        BrokerImage::from_manifest(&manifest.parse::<DocumentMut>().unwrap())
    }

    #[test]
    fn the_image_is_published_under_the_owner_of_the_source_repository() {
        let image = image("[workspace.package]\nrepository = \"https://github.com/Example/andromeda\"\n").unwrap();
        assert_eq!(image.purl(ImageForm::Oci), "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda");
        assert_eq!(image.purl(ImageForm::Docker), "pkg:docker/example/andromeda?repository_url=ghcr.io");
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
    fn only_a_package_url_of_the_image_without_a_version_or_a_narrowing_qualifier_names_it() {
        let image = image("[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n").unwrap();
        for (id, form) in [
            ("pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda", Some(ImageForm::Oci)),
            ("pkg:oci/andromeda?repository_url=ghcr.io%2Fexample%2Fandromeda", Some(ImageForm::Oci)),
            ("pkg:docker/example/andromeda?repository_url=ghcr.io", Some(ImageForm::Docker)),
            ("pkg:oci/andromeda?arch=amd64&repository_url=ghcr.io/example/andromeda", None),
            ("pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda&tag=1.0.0", None),
            ("pkg:docker/example/andromeda?arch=arm64&repository_url=ghcr.io", None),
            ("pkg:oci/andromedaa?repository_url=ghcr.io/example/andromeda", None),
            ("pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda", None),
            ("pkg:oci/andromeda?repository_url=docker.io/example/andromeda", None),
            ("pkg:oci/andromeda", None),
            ("pkg:oci/andromeda@sha256:0123?repository_url=ghcr.io/example/andromeda", None),
            ("pkg:oci/example/andromeda?repository_url=ghcr.io/example/andromeda", None),
            ("pkg:docker/andromeda?repository_url=ghcr.io/example/andromeda", None),
            ("pkg:docker/other/andromeda?repository_url=ghcr.io", None),
            ("pkg:docker/example/andromeda", None),
            ("pkg:docker/example/andromeda?repository_url=docker.io", None),
            ("pkg:docker/example/andromeda@probe?repository_url=ghcr.io", None),
            ("pkg:docker/ghcr.io/example/andromeda", None),
            ("ghcr.io/example/andromeda", None),
        ] {
            assert_eq!(image.form_named_by(id), form, "{id}");
        }
    }
}
