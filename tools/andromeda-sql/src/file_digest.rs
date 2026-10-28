//! The SHA-256 digest of a file's raw bytes, written as 64 lowercase
//! hexadecimal digits.

use serde_with::{DeserializeFromStr, SerializeDisplay};
use sha2::{Digest, Sha256};
use std::str::FromStr;

/// The SHA-256 digest of the bytes of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display, SerializeDisplay, DeserializeFromStr)]
#[display("{}", hex::encode(_0))]
pub struct FileDigest([u8; 32]);

impl FileDigest {
    /// The digest of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> FileDigest {
        FileDigest(Sha256::digest(bytes).into())
    }
}

impl FromStr for FileDigest {
    type Err = FileDigestError;

    /// Read a digest written as 64 lowercase hexadecimal digits.
    fn from_str(written: &str) -> Result<FileDigest, FileDigestError> {
        if written.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return Err(FileDigestError::Uppercase);
        }
        let mut digest = [0; 32];
        hex::decode_to_slice(written, &mut digest).map_err(FileDigestError::NotHexadecimal)?;
        Ok(FileDigest(digest))
    }
}

/// Why a written digest could not be read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FileDigestError {
    /// The text is not 64 hexadecimal digits.
    #[error("a SHA-256 digest is 64 hexadecimal digits: {0}")]
    NotHexadecimal(hex::FromHexError),
    /// The text holds an uppercase digit.
    #[error("a SHA-256 digest is written in lowercase hexadecimal")]
    Uppercase,
}

#[cfg(test)]
mod tests {
    use crate::file_digest::{FileDigest, FileDigestError};
    use pretty_assertions::assert_eq;

    /// The SHA-256 of `abc`, from FIPS 180-2 appendix B.1.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn the_digest_of_bytes_is_written_in_lowercase_hexadecimal_and_reads_back() {
        let digest = FileDigest::of(b"abc");
        assert_eq!(digest.to_string(), ABC);
        assert_eq!(ABC.parse::<FileDigest>(), Ok(digest));
    }

    #[test]
    fn a_digest_in_uppercase_or_of_the_wrong_length_is_refused() {
        assert_eq!(ABC.to_uppercase().parse::<FileDigest>(), Err(FileDigestError::Uppercase));
        assert!(matches!(ABC[2..].parse::<FileDigest>(), Err(FileDigestError::NotHexadecimal(_))));
    }
}
