//! The protocol settings every accepted TCP connection is served under.

use crate::field_section::SMALLEST_FIELD;
use andromeda_configuration::header_section_limit::HeaderSectionLimit;
use hyper_util::{rt::TokioExecutor, server::conn::auto::Builder};

/// A connection builder speaking HTTP/1.1 or HTTP/2, as the client chooses,
/// under the request header section `limit`.
///
/// HTTP/2 refuses a section whose RFC 9113 section 6.5.2 size reaches its
/// setting, so the setting is one octet over the limit. HTTP/1.1 reads a
/// request head into a buffer whose ceiling is the limit: a head within the
/// limit is smaller than its measured size, since each field costs 4 raw
/// octets against 32, so only a head over the limit fills the buffer and is
/// refused 431 there; the rest are measured by
/// [`refuse_oversized_http1_section`](crate::field_section::refuse_oversized_http1_section).
/// Its count of header lines is raised to the most fields a section within
/// the limit can hold, so the count never refuses what HTTP/2 accepts.
pub(crate) fn connection_builder(limit: HeaderSectionLimit) -> Builder<TokioExecutor> {
    let octets = limit.octets();
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .max_buf_size(usize::from(octets))
        .max_headers(usize::from(octets) / SMALLEST_FIELD);
    builder.http2().max_header_list_size(u32::from(octets) + 1);
    builder
}
