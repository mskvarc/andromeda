//! The files a validated `[tls]` section loads the broker's identity from.

use std::{
    borrow::Cow,
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Which client certificates a listener asks for, and the CA bundle they must
/// chain to when it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientTrust<'config> {
    /// No client certificate is requested.
    Anonymous,
    /// A certificate chaining to the bundle is verified when presented; a
    /// client presenting none is still served.
    Optional(Cow<'config, Path>),
    /// A certificate chaining to the bundle is required.
    Required(Cow<'config, Path>),
}

impl ClientTrust<'_> {
    /// The CA bundle client certificates are verified against, when any are requested.
    #[must_use]
    pub fn bundle_path(&self) -> Option<&Path> {
        match self {
            ClientTrust::Anonymous => None,
            ClientTrust::Optional(path) | ClientTrust::Required(path) => Some(path),
        }
    }

    /// Detach from the configuration the paths were borrowed from.
    fn into_owned(self) -> ClientTrust<'static> {
        match self {
            ClientTrust::Anonymous => ClientTrust::Anonymous,
            ClientTrust::Optional(path) => ClientTrust::Optional(Cow::Owned(path.into_owned())),
            ClientTrust::Required(path) => ClientTrust::Required(Cow::Owned(path.into_owned())),
        }
    }
}

/// The certificate, key and optional client CA files of a validated `[tls]`
/// section.
///
/// Only [`TlsConfig::material_sources`](crate::config::TlsConfig::material_sources)
/// constructs one, so holding one is the proof that the section names a
/// complete identity: a caller never re-checks for empty paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsMaterialSources<'config> {
    /// PEM certificate chain, leaf first.
    certificate_path: Cow<'config, Path>,
    /// PEM private key for the leaf.
    private_key_path: Cow<'config, Path>,
    /// Which client certificates are requested, and against which bundle.
    client_trust: ClientTrust<'config>,
}

impl<'config> TlsMaterialSources<'config> {
    /// Bundle the validated paths.
    pub(crate) const fn new(certificate_path: Cow<'config, Path>, private_key_path: Cow<'config, Path>, client_trust: ClientTrust<'config>) -> Self {
        TlsMaterialSources {
            certificate_path,
            private_key_path,
            client_trust,
        }
    }

    /// The PEM certificate chain path.
    #[must_use]
    pub fn certificate_path(&self) -> &Path {
        &self.certificate_path
    }

    /// The PEM private key path.
    #[must_use]
    pub fn private_key_path(&self) -> &Path {
        &self.private_key_path
    }

    /// Which client certificates are requested.
    #[must_use]
    pub const fn client_trust(&self) -> &ClientTrust<'config> {
        &self.client_trust
    }

    /// The directories holding the configured files, each named once.
    ///
    /// The directories rather than the files are watched: a Kubernetes Secret
    /// volume rotates by renaming its `..data` symlink and certbot by renaming
    /// a new file into place, and neither ever writes to the path the broker
    /// was configured with. A bare file name lives in the working directory.
    #[must_use]
    pub fn watched_directories(&self) -> BTreeSet<PathBuf> {
        [Some(self.certificate_path()), Some(self.private_key_path()), self.client_trust.bundle_path()]
            .into_iter()
            .flatten()
            .map(|file| match file.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                Some(_) | None => PathBuf::from("."),
            })
            .collect()
    }

    /// Detach from the configuration the paths were borrowed from, so a
    /// reload running on the watcher's own thread can own them.
    #[must_use]
    pub fn into_owned(self) -> TlsMaterialSources<'static> {
        TlsMaterialSources {
            certificate_path: Cow::Owned(self.certificate_path.into_owned()),
            private_key_path: Cow::Owned(self.private_key_path.into_owned()),
            client_trust: self.client_trust.into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::material_sources::{ClientTrust, TlsMaterialSources};
    use pretty_assertions::assert_eq;
    use std::{
        borrow::Cow,
        collections::BTreeSet,
        path::{Path, PathBuf},
    };

    #[test]
    fn files_sharing_a_directory_watch_it_once() {
        let sources = TlsMaterialSources::new(
            Cow::Borrowed(Path::new("/etc/andromeda/tls/broker.crt")),
            Cow::Borrowed(Path::new("/etc/andromeda/tls/broker.key")),
            ClientTrust::Required(Cow::Borrowed(Path::new("/etc/andromeda/clients/ca.pem"))),
        );
        let expected: BTreeSet<PathBuf> = [PathBuf::from("/etc/andromeda/clients"), PathBuf::from("/etc/andromeda/tls")].into();
        assert_eq!(sources.watched_directories(), expected);
    }

    #[test]
    fn a_bare_file_name_watches_the_working_directory() {
        let sources = TlsMaterialSources::new(
            Cow::Borrowed(Path::new("broker.crt")),
            Cow::Borrowed(Path::new("broker.key")),
            ClientTrust::Anonymous,
        );
        assert_eq!(sources.watched_directories(), BTreeSet::from([PathBuf::from(".")]));
    }

    #[test]
    fn owned_sources_equal_the_borrowed_ones() {
        let sources = TlsMaterialSources::new(
            Cow::Borrowed(Path::new("/tls/broker.crt")),
            Cow::Borrowed(Path::new("/tls/broker.key")),
            ClientTrust::Optional(Cow::Borrowed(Path::new("/tls/ca.pem"))),
        );
        assert_eq!(sources.clone().into_owned(), sources);
    }
}
