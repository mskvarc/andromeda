//! A certificate authority that issues throwaway server and client leaves.

use crate::{error::TestTlsError, identity::TestIdentity};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;

/// The names a server leaf is valid for: the loopback host a test dials.
const SERVER_NAMES: [&str; 2] = ["localhost", "127.0.0.1"];

/// A self-signed authority whose certificate a test client or server trusts.
///
/// Each generated authority has a fresh key, so two authorities never trust
/// each other's leaves: a test of an unknown issuer generates a second one.
pub struct TestAuthority {
    /// The authority's certificate and the key it signs leaves with.
    issuer: CertifiedIssuer<'static, KeyPair>,
}

impl TestAuthority {
    /// Generate a fresh authority.
    ///
    /// # Errors
    ///
    /// Returns [`TestTlsError::Generation`] when the key or certificate cannot
    /// be generated.
    pub fn generate() -> Result<TestAuthority, TestTlsError> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.distinguished_name.push(DnType::CommonName, "Andromeda test authority");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
        let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate()?)?;
        Ok(TestAuthority { issuer })
    }

    /// The authority's DER certificate.
    #[must_use]
    pub fn certificate(&self) -> &CertificateDer<'static> {
        self.issuer.der()
    }

    /// The authority's PEM certificate, as a client CA bundle holds it.
    #[must_use]
    pub fn certificate_pem(&self) -> String {
        self.issuer.pem()
    }

    /// A root store trusting only this authority.
    ///
    /// # Errors
    ///
    /// Returns [`TestTlsError::Trust`] when the certificate is not a usable
    /// trust anchor.
    pub fn root_store(&self) -> Result<RootCertStore, TestTlsError> {
        let mut roots = RootCertStore::empty();
        roots.add(self.certificate().clone())?;
        Ok(roots)
    }

    /// Issue a server leaf valid for `localhost` and `127.0.0.1`.
    ///
    /// # Errors
    ///
    /// Returns [`TestTlsError::Generation`] when the leaf cannot be generated.
    pub fn issue_server_identity(&self) -> Result<TestIdentity, TestTlsError> {
        let mut params = CertificateParams::new(SERVER_NAMES.map(str::to_owned).to_vec())?;
        params.distinguished_name.push(DnType::CommonName, "localhost");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        self.issue(&params)
    }

    /// Issue a client leaf for mutual TLS.
    ///
    /// # Errors
    ///
    /// Returns [`TestTlsError::Generation`] when the leaf cannot be generated.
    pub fn issue_client_identity(&self) -> Result<TestIdentity, TestTlsError> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.distinguished_name.push(DnType::CommonName, "Andromeda test client");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        self.issue(&params)
    }

    /// Sign a leaf for `params` under a fresh key.
    fn issue(&self, params: &CertificateParams) -> Result<TestIdentity, TestTlsError> {
        let key = KeyPair::generate()?;
        let certificate = params.signed_by(&key, &self.issuer)?;
        Ok(TestIdentity::new(certificate.der().clone(), certificate.pem(), &key))
    }
}

#[cfg(test)]
mod tests {
    use crate::authority::TestAuthority;
    use std::error::Error;

    #[test]
    fn a_generated_authority_is_a_usable_trust_anchor() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        pretty_assertions::assert_eq!(authority.root_store()?.len(), 1);
        assert!(authority.certificate_pem().starts_with("-----BEGIN CERTIFICATE-----"));
        Ok(())
    }

    #[test]
    fn two_issued_leaves_differ() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        assert_ne!(first.certificate(), second.certificate());
        assert_ne!(first.private_key_pem(), second.private_key_pem());
        Ok(())
    }
}
