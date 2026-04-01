use crate::query::error::QueryError;
use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};

use crate::error::ngsi_ld::{NgsiLdError, NgsiLdErrorType};

#[derive(Debug)]
pub enum ExtractorRejection {
    Empty(StatusCode),
    InvalidRequest { detail: String, instance: Uri },
    BadRequestData { detail: String, instance: Uri },
}

impl ExtractorRejection {
    pub fn invalid_request(detail: impl Into<String>, instance: Uri) -> Self {
        Self::InvalidRequest {
            detail: detail.into(),
            instance,
        }
    }

    pub fn bad_request_data(detail: impl Into<String>, instance: Uri) -> Self {
        Self::BadRequestData {
            detail: detail.into(),
            instance,
        }
    }

    pub fn from_query_error(err: QueryError, instance: Uri) -> Self {
        match err {
            QueryError::BadRequestData(detail) => Self::BadRequestData { detail, instance },
            QueryError::InvalidRequest(detail) => Self::InvalidRequest { detail, instance },
        }
    }
}

impl IntoResponse for ExtractorRejection {
    fn into_response(self) -> Response {
        match self {
            ExtractorRejection::Empty(status) => status.into_response(),
            ExtractorRejection::InvalidRequest { detail, instance } => {
                let ngsi = NgsiLdError::new(NgsiLdErrorType::InvalidRequest, detail, instance);
                ngsi.into_response()
            }
            ExtractorRejection::BadRequestData { detail, instance } => {
                let ngsi = NgsiLdError::new(NgsiLdErrorType::BadRequestData, detail, instance);
                ngsi.into_response()
            }
        }
    }
}
