//! Telling an HTTP/2 connection from an HTTP/1.1 one by the first octets it
//! received.
//!
//! RFC 9113 section 3.4: an HTTP/2 connection opens with the client
//! connection preface, `PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`, over plaintext and
//! over TLS alike, so the first 24 octets tell the two protocols apart before
//! a request has been read. Octets that leave the preface are HTTP/1.1, and
//! octets that match it so far leave the protocol open.

/// The client connection preface every HTTP/2 connection opens with.
const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// The protocol a connection speaks, as far as its first octets tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrefaceReading {
    /// The octets received so far, `matched` of them, all match the preface.
    Undecided {
        /// How many octets of the preface have arrived.
        matched: usize,
    },
    /// The connection opened with the whole preface.
    Http2,
    /// The connection's octets left the preface: it speaks HTTP/1.1.
    Http1,
}

impl PrefaceReading {
    /// The reading of a connection that has received nothing yet.
    pub(crate) const NOTHING_RECEIVED: PrefaceReading = PrefaceReading::Undecided { matched: 0 };

    /// The reading after `octets` arrived next on the connection.
    pub(crate) fn after(self, octets: &[u8]) -> PrefaceReading {
        match self {
            PrefaceReading::Undecided { matched } => {
                let expected = PREFACE.get(matched..).unwrap_or_default();
                let compared = octets.len().min(expected.len());
                if octets.get(..compared) != expected.get(..compared) {
                    PrefaceReading::Http1
                } else if matched.saturating_add(compared) == PREFACE.len() {
                    PrefaceReading::Http2
                } else {
                    PrefaceReading::Undecided {
                        matched: matched.saturating_add(compared),
                    }
                }
            }
            decided @ (PrefaceReading::Http2 | PrefaceReading::Http1) => decided,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::http2_preface::PrefaceReading;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_connection_that_received_nothing_is_undecided() {
        assert_eq!(PrefaceReading::NOTHING_RECEIVED.after(b""), PrefaceReading::Undecided { matched: 0 });
    }

    #[test]
    fn the_whole_preface_in_one_read_is_http2() {
        assert_eq!(
            PrefaceReading::NOTHING_RECEIVED.after(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\x00\x00\x12\x04"),
            PrefaceReading::Http2
        );
    }

    #[test]
    fn the_preface_split_across_reads_is_http2() {
        let reading = PrefaceReading::NOTHING_RECEIVED.after(b"PRI * HT");
        assert_eq!(reading, PrefaceReading::Undecided { matched: 8 });
        assert_eq!(reading.after(b"TP/2.0\r\n\r\nSM\r\n\r\n"), PrefaceReading::Http2);
    }

    #[test]
    fn a_request_line_is_http1() {
        assert_eq!(PrefaceReading::NOTHING_RECEIVED.after(b"GET / HTTP/1.1\r\n"), PrefaceReading::Http1);
    }

    #[test]
    fn a_request_line_sharing_the_first_octet_of_the_preface_is_http1_at_the_first_difference() {
        let reading = PrefaceReading::NOTHING_RECEIVED.after(b"P");
        assert_eq!(reading, PrefaceReading::Undecided { matched: 1 });
        assert_eq!(reading.after(b"UT /x HTTP/1.1\r\n"), PrefaceReading::Http1);
    }

    #[test]
    fn a_decided_reading_stays_decided() {
        assert_eq!(PrefaceReading::Http2.after(b"GET / HTTP/1.1\r\n"), PrefaceReading::Http2);
        assert_eq!(PrefaceReading::Http1.after(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"), PrefaceReading::Http1);
    }
}
