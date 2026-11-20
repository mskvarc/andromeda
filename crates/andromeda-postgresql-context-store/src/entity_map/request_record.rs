//! The three `entity_maps` columns recording the request that created a map,
//! in the form the table holds them.

use crate::entity_map::{
    mapper::StoredEntityMapError,
    request_kind_code::{request_kind, stored_kind},
};
use andromeda_ngsi_ld_model::entity_map_fingerprint::{QueryFingerprint, REQUEST_DIGEST_BYTES, RequestFingerprint};
use andromeda_postgresql_stored_values::entity_map_request_kind::EntityMapRequestKindCode;

/// The `request_kind`, `query_digest` and `inputs_digest` columns of one map.
///
/// The table holds all three or none of them; a value read back with only some
/// set is not a record the broker wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestRecord {
    /// The stored code of the operation that created the map.
    pub(crate) kind: Option<i16>,
    /// The SHA-256 digest of the creating request's compiled query.
    pub(crate) query_digest: Option<Vec<u8>>,
    /// The SHA-256 digest of the creating request's other inputs.
    pub(crate) inputs_digest: Option<Vec<u8>>,
}

impl RequestRecord {
    /// The columns recording `fingerprint`, or the unset columns of a map
    /// created without a recorded request.
    pub(crate) fn of(fingerprint: Option<&RequestFingerprint>) -> RequestRecord {
        RequestRecord {
            kind: fingerprint.map(|fingerprint| stored_kind(fingerprint.kind()).code()),
            query_digest: fingerprint.map(|fingerprint| fingerprint.query().digest().to_vec()),
            inputs_digest: fingerprint.map(|fingerprint| fingerprint.inputs().to_vec()),
        }
    }

    /// The fingerprint the columns record, or `None` when none of them is set.
    pub(crate) fn fingerprint(&self) -> Result<Option<RequestFingerprint>, StoredEntityMapError> {
        match (self.kind, self.query_digest.as_deref(), self.inputs_digest.as_deref()) {
            (None, None, None) => Ok(None),
            (Some(kind), Some(query), Some(inputs)) => {
                let kind = request_kind(EntityMapRequestKindCode::try_from(kind).map_err(StoredEntityMapError::RequestKind)?);
                let query = <[u8; REQUEST_DIGEST_BYTES]>::try_from(query).map_err(|_| StoredEntityMapError::QueryDigestLength(query.len()))?;
                let inputs = <[u8; REQUEST_DIGEST_BYTES]>::try_from(inputs).map_err(|_| StoredEntityMapError::InputsDigestLength(inputs.len()))?;
                Ok(Some(RequestFingerprint::new(QueryFingerprint::new(kind, query), inputs)))
            }
            (Some(_), _, _) | (_, Some(_), _) | (_, _, Some(_)) => Err(StoredEntityMapError::PartialRequestRecord),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map::{mapper::StoredEntityMapError, request_record::RequestRecord};
    use andromeda_ngsi_ld_model::entity_map_fingerprint::{MapRequestKind, QueryFingerprint, RequestFingerprint};
    use pretty_assertions::assert_eq;

    fn fingerprint() -> RequestFingerprint {
        RequestFingerprint::new(QueryFingerprint::new(MapRequestKind::RetrieveTemporal, [0xab; 32]), [0xcd; 32])
    }

    #[test]
    fn a_recorded_request_is_written_as_its_code_and_both_digests_and_reads_back() {
        let record = RequestRecord::of(Some(&fingerprint()));
        assert_eq!(record.kind, Some(4));
        assert_eq!(record.query_digest, Some(vec![0xab; 32]));
        assert_eq!(record.inputs_digest, Some(vec![0xcd; 32]));
        assert_eq!(record.fingerprint().expect("decodes"), Some(fingerprint()));
    }

    #[test]
    fn a_map_without_a_recorded_request_writes_and_reads_no_columns() {
        let record = RequestRecord::of(None);
        assert_eq!(
            record,
            RequestRecord {
                kind: None,
                query_digest: None,
                inputs_digest: None
            }
        );
        assert_eq!(record.fingerprint().expect("decodes"), None);
    }

    #[test]
    fn an_unassigned_kind_code_is_refused() {
        for code in [0, 5] {
            let record = RequestRecord {
                kind: Some(code),
                ..RequestRecord::of(Some(&fingerprint()))
            };
            assert!(matches!(record.fingerprint(), Err(StoredEntityMapError::RequestKind(_))), "{code}");
        }
    }

    #[test]
    fn a_digest_that_is_not_32_bytes_is_refused() {
        let query = RequestRecord {
            query_digest: Some(vec![0; 31]),
            ..RequestRecord::of(Some(&fingerprint()))
        };
        assert!(matches!(query.fingerprint(), Err(StoredEntityMapError::QueryDigestLength(31))));
        let inputs = RequestRecord {
            inputs_digest: Some(vec![0; 33]),
            ..RequestRecord::of(Some(&fingerprint()))
        };
        assert!(matches!(inputs.fingerprint(), Err(StoredEntityMapError::InputsDigestLength(33))));
    }

    #[test]
    fn a_record_with_only_some_columns_set_is_refused() {
        let full = RequestRecord::of(Some(&fingerprint()));
        let partial = [
            RequestRecord { kind: None, ..full.clone() },
            RequestRecord {
                query_digest: None,
                ..full.clone()
            },
            RequestRecord { inputs_digest: None, ..full },
            RequestRecord {
                kind: Some(1),
                query_digest: None,
                inputs_digest: None,
            },
        ];
        for record in partial {
            assert!(matches!(record.fingerprint(), Err(StoredEntityMapError::PartialRequestRecord)), "{record:?}");
        }
    }
}
