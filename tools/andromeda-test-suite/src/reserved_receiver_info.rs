//! The `receiverInfo` entries that name a header field the broker writes on every HTTP notification itself.

/// Each header field a notification carries as the broker and its HTTP client write it, as a `receiverInfo` key with a
/// value of the kind that field holds. The keys are spelled in mixed letter cases, since field names compare without
/// regard to case (IETF RFC 9110 section 5.1).
pub const RESERVED_RECEIVER_INFO: [(&str, &str); 9] = [
    ("Content-Type", "text/plain"),
    ("content-length", "5"),
    ("Transfer-Encoding", "chunked"),
    ("HOST", "elsewhere.example"),
    ("Connection", "close"),
    ("te", "trailers"),
    ("Trailer", "Expires"),
    ("UPGRADE", "websocket"),
    ("Keep-Alive", "timeout=5"),
];
