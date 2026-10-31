//! The leaf certificate a TLS server presented for a response (clause 6.2), read from what the transport attached to
//! the response into the fields a test compares: distinguished names in the OpenSSL one-line form (`CN = localhost`)
//! and the serial number as lower-case hexadecimal octets joined by `:`. The client must be built to keep TLS
//! information ([`crate::tls_connection::Connection`] does).

use http::Extensions;
use reqwest::tls::TlsInfo;
use x509_parser::{
    certificate::X509Certificate,
    error::X509Error,
    nom,
    objects::{oid_registry, oid2abbrev},
    prelude::FromDer,
    x509::X509Name,
};

/// Why a response carries no readable presented certificate.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PresentedCertificateError {
    /// The response came over a transport that kept no TLS information, such as plain HTTP.
    #[error("the response carries no TLS information")]
    NoTlsInformation,
    /// The TLS information holds no peer certificate.
    #[error("the TLS information holds no peer certificate")]
    NoCertificate,
    /// The presented certificate is not DER X.509.
    #[error("the presented certificate is not DER X.509")]
    NotX509(#[source] nom::Err<X509Error>),
}

/// The fields of the certificate a server presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedCertificate {
    /// The subject's distinguished name.
    subject: String,
    /// The issuer's distinguished name.
    issuer: String,
    /// The serial number.
    serial_number: String,
}

impl PresentedCertificate {
    /// The certificate the server presented for the response whose transport attached `extensions`.
    ///
    /// # Errors
    ///
    /// Returns [`PresentedCertificateError`] when the response keeps no TLS information or no readable certificate.
    pub fn of(extensions: &Extensions) -> Result<PresentedCertificate, PresentedCertificateError> {
        let der = extensions
            .get::<TlsInfo>()
            .ok_or(PresentedCertificateError::NoTlsInformation)?
            .peer_certificate()
            .ok_or(PresentedCertificateError::NoCertificate)?;
        let (_, certificate) = X509Certificate::from_der(der).map_err(PresentedCertificateError::NotX509)?;
        Ok(PresentedCertificate {
            subject: one_line(certificate.subject()),
            issuer: one_line(certificate.issuer()),
            serial_number: serial(certificate.raw_serial()),
        })
    }

    /// The subject's distinguished name.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The issuer's distinguished name.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The serial number.
    #[must_use]
    pub fn serial_number(&self) -> &str {
        &self.serial_number
    }
}

/// `name` in the OpenSSL one-line form: each attribute as `ABBREVIATION = value`, attributes of one relative name
/// joined by ` + `, relative names by `, `.
fn one_line(name: &X509Name<'_>) -> String {
    let registry = oid_registry();
    name.iter_rdn()
        .map(|rdn| {
            rdn.iter()
                .map(|attribute| {
                    let key = oid2abbrev(attribute.attr_type(), registry).map_or_else(|_| attribute.attr_type().to_id_string(), str::to_owned);
                    let value = attribute.as_str().map_or_else(|_| String::new(), str::to_owned);
                    format!("{key} = {value}")
                })
                .collect::<Vec<_>>()
                .join(" + ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The serial number `raw` (the DER integer's content octets) as lower-case hexadecimal octets joined by `:`, without
/// the zero octet DER puts before a first octet whose high bit is set.
fn serial(raw: &[u8]) -> String {
    let significant = match raw {
        [0, rest @ ..] if !rest.is_empty() => rest,
        whole => whole,
    };
    significant.iter().map(|octet| format!("{octet:02x}")).collect::<Vec<_>>().join(":")
}

#[cfg(test)]
mod tests {
    use crate::presented_certificate::{PresentedCertificate, PresentedCertificateError, serial};
    use googletest::prelude::*;
    use http::Extensions;

    #[gtest]
    fn a_serial_number_drops_the_sign_octet() {
        expect_that!(serial(&[0x00, 0xdb, 0x64, 0x05]), eq("db:64:05"));
    }

    #[gtest]
    fn a_serial_number_of_one_zero_octet_is_kept() {
        expect_that!(serial(&[0x00]), eq("00"));
    }

    #[gtest]
    fn a_response_over_a_transport_without_tls_information_is_refused() {
        expect_that!(
            PresentedCertificate::of(&Extensions::new()),
            err(eq(&PresentedCertificateError::NoTlsInformation))
        );
    }
}
