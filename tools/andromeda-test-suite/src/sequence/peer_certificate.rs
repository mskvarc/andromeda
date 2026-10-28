//! The certificate a TLS server presented, read into the fields a check compares, written as Hurl writes them:
//! distinguished names in the OpenSSL one-line form (`CN = localhost`), the serial number as lower-case hexadecimal
//! octets joined by `:`.

use x509_parser::{
    certificate::X509Certificate,
    objects::{oid_registry, oid2abbrev},
    prelude::FromDer,
    x509::X509Name,
};

/// Why a presented certificate cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the presented certificate is not DER X.509: {0}")]
pub struct CertificateError(String);

/// A field of a certificate a check reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum CertificateField {
    /// The subject's distinguished name.
    Subject,
    /// The issuer's distinguished name.
    Issuer,
    /// The serial number.
    #[strum(serialize = "Serial-Number")]
    SerialNumber,
}

/// The fields of the certificate a server presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCertificate {
    /// The subject's distinguished name.
    subject: String,
    /// The issuer's distinguished name.
    issuer: String,
    /// The serial number.
    serial_number: String,
}

impl PeerCertificate {
    /// The certificate encoded as `der`.
    ///
    /// # Errors
    ///
    /// Returns [`CertificateError`] when `der` is not a DER-encoded X.509 certificate.
    pub fn from_der(der: &[u8]) -> Result<PeerCertificate, CertificateError> {
        let (_, certificate) = X509Certificate::from_der(der).map_err(|error| CertificateError(error.to_string()))?;
        Ok(PeerCertificate {
            subject: one_line(certificate.subject()),
            issuer: one_line(certificate.issuer()),
            serial_number: serial(certificate.raw_serial()),
        })
    }

    /// The text of `field`.
    #[must_use]
    pub fn field(&self, field: CertificateField) -> &str {
        match field {
            CertificateField::Subject => &self.subject,
            CertificateField::Issuer => &self.issuer,
            CertificateField::SerialNumber => &self.serial_number,
        }
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

/// The serial number `raw` (the DER integer's content octets) as lower-case hexadecimal octets joined by `:`,
/// without the zero octet DER puts before a first octet whose high bit is set.
fn serial(raw: &[u8]) -> String {
    let significant = match raw {
        [0, rest @ ..] if !rest.is_empty() => rest,
        whole => whole,
    };
    significant.iter().map(|octet| format!("{octet:02x}")).collect::<Vec<_>>().join(":")
}

#[cfg(test)]
mod tests {
    use crate::sequence::peer_certificate::serial;
    use googletest::prelude::*;

    #[gtest]
    fn a_serial_number_drops_the_sign_octet() {
        expect_that!(serial(&[0x00, 0xdb, 0x64, 0x05]), eq("db:64:05"));
    }

    #[gtest]
    fn a_serial_number_of_one_zero_octet_is_kept() {
        expect_that!(serial(&[0x00]), eq("00"));
    }
}
