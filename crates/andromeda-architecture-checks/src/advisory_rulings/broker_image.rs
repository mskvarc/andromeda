//! The broker image as scanners name it, the products every VEX statement
//! names so every scanner applies it.
//!
//! The release publishes the image to the GitHub container registry under the
//! owner of the source repository, as `ghcr.io/<owner>/andromeda`. Scanners
//! name a pulled image in one of two package URL forms. The `oci` form names
//! it by the last segment of its repository, with the whole repository as the
//! `repository_url` qualifier. The `docker` form names it by its owner and
//! name, with the registry as the `repository_url` qualifier. Either carries
//! the digest as the version, and a scanner applies a statement when one of its
//! products names the same image in the scanner's form with a subset of those
//! qualifiers. So a product with no version and no qualifier but
//! `repository_url` covers every digest, which is the one the document names in
//! each form.

use crate::advisory_rulings::source_repository::SourceRepository;
use strum::EnumIter;

/// The registry the release publishes the broker image to.
const IMAGE_REGISTRY: &str = "ghcr.io";

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

/// The package URLs that name every digest of the broker image.
#[derive(Debug)]
pub(crate) struct BrokerImage {
    /// The `oci` package URL naming every digest of the image.
    oci: String,
    /// The `docker` package URL naming every digest of the image.
    docker: String,
}

impl BrokerImage {
    /// The image the owner of `source` publishes.
    pub(crate) fn published_by(source: &SourceRepository) -> BrokerImage {
        let owner = source.owner();
        BrokerImage {
            oci: format!("pkg:{OCI_TYPE}/{IMAGE_NAME}?{REPOSITORY_QUALIFIER}={IMAGE_REGISTRY}/{owner}/{IMAGE_NAME}"),
            docker: format!("pkg:{DOCKER_TYPE}/{owner}/{IMAGE_NAME}?{REPOSITORY_QUALIFIER}={IMAGE_REGISTRY}"),
        }
    }

    /// The package URL in `form` naming every digest of the image.
    pub(crate) fn purl(&self, form: ImageForm) -> &str {
        match form {
            ImageForm::Oci => &self.oci,
            ImageForm::Docker => &self.docker,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::{
        broker_image::{BrokerImage, ImageForm},
        source_repository::SourceRepository,
    };
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    #[test]
    fn the_image_is_published_under_the_owner_of_the_source_repository() {
        let manifest = "[workspace.package]\nrepository = \"https://github.com/Example/andromeda\"\n"
            .parse::<DocumentMut>()
            .unwrap();
        let image = BrokerImage::published_by(&SourceRepository::from_manifest(&manifest).unwrap());
        assert_eq!(image.purl(ImageForm::Oci), "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda");
        assert_eq!(image.purl(ImageForm::Docker), "pkg:docker/example/andromeda?repository_url=ghcr.io");
    }
}
