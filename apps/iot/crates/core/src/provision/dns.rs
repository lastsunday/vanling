//! The captive portal's half of DNS: answer every `A` query with this device's own
//! address, so a phone that cannot reach the internet opens the page instead.
//!
//! Not a resolver. Nothing is looked up, nothing is cached, and an address is the
//! only record this can write. That is the whole trick: a phone's connectivity check
//! asks for a name that can never be reached here, gets an answer it can, and offers
//! its user the page on the other end.
//!
//! The answer copies the question's ID and its own question section verbatim, which
//! is what lets a client match the reply to what it asked — and what makes this
//! correct for a name this code never parses.

/// The only record type this answers.
const TYPE_A: u16 = 1;
/// The only class this answers. `CH` and the rest are not something a phone asks for.
const CLASS_IN: u16 = 1;
/// Nothing here may be cached: the answer is about *this* network, and a client that
/// kept it would open a stale page on the next one.
const TTL_SECONDS: u32 = 0;

/// Write an answer to `query` into `out`, returning how many bytes it takes.
///
/// `None` when `out` is too small, when the query is not a query, or when the
/// question is truncated. A query that *is* one but asks for something other than an
/// `A` record still gets a reply — with no records in it — because a client that sent
/// an `AAAA` query is a client waiting for an answer, and silence is a timeout it
/// will retry rather than fall back from.
pub fn answer(query: &[u8], address: [u8; 4], out: &mut [u8]) -> Option<usize> {
    if query.len() < 12 {
        return None;
    }
    let questions = u16::from_be_bytes([query[4], query[5]]);
    if questions == 0 {
        return None;
    }
    let question_end = skip_question(query)?;
    let qtype = u16::from_be_bytes([query[question_end - 4], query[question_end - 3]]);
    let qclass = u16::from_be_bytes([query[question_end - 2], query[question_end - 1]]);
    let answers = u16::from(u8::from(qtype == TYPE_A && qclass == CLASS_IN));

    // A name pointer to the question's own name at offset 12, then the fixed-length
    // A record: type, class, TTL, and the four octets.
    let answer_len = if answers == 1 { 16 } else { 0 };
    let total = question_end + answer_len;
    if out.len() < total {
        return None;
    }

    out[0] = query[0];
    out[1] = query[1];
    // QR set, AA set, the client's own RD echoed, RA set, RCODE 0.
    out[2] = 0x84 | (query[2] & 0x01);
    out[3] = 0x80;
    out[4..6].copy_from_slice(&1u16.to_be_bytes());
    out[6..8].copy_from_slice(&answers.to_be_bytes());
    out[8..12].fill(0);
    out[12..question_end].copy_from_slice(&query[12..question_end]);

    if answers == 1 {
        let record = &mut out[question_end..];
        record[0] = 0xc0;
        record[1] = 0x0c;
        record[2..4].copy_from_slice(&TYPE_A.to_be_bytes());
        record[4..6].copy_from_slice(&CLASS_IN.to_be_bytes());
        record[6..10].copy_from_slice(&TTL_SECONDS.to_be_bytes());
        record[10..12].copy_from_slice(&4u16.to_be_bytes());
        record[12..16].copy_from_slice(&address);
    }
    Some(total)
}

/// The offset just past the first question's name, type and class.
///
/// A query's name is uncompressed — a client has nothing earlier in the message to
/// point at — so a label that claims to be a pointer is a malformed one, and a label
/// longer than a name allows is too. Both are refused rather than followed.
fn skip_question(query: &[u8]) -> Option<usize> {
    let mut index = 12;
    loop {
        let length = *query.get(index)? as usize;
        if length == 0 {
            index += 1;
            break;
        }
        if length > 63 {
            return None;
        }
        index = index.checked_add(1 + length)?;
    }
    index.checked_add(4).filter(|end| *end <= query.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A query for one name, with the header flags a client sets.
    fn query(labels: &[&[u8]], qtype: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        for label in labels {
            out.push(label.len() as u8);
            out.extend_from_slice(label);
        }
        out.push(0);
        out.extend_from_slice(&qtype.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out
    }

    #[test]
    fn an_a_query_is_answered_with_the_devices_own_address() {
        let ask = query(&[b"connectivity", b"example"], TYPE_A);
        let mut reply = [0u8; 128];
        let len = answer(&ask, [192, 168, 4, 1], &mut reply).expect("room enough");

        assert_eq!(&reply[..2], &[0x12, 0x34], "the client's own id comes back");
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 1, "one answer");
        // The question is echoed byte for byte, which is what a client matches on.
        assert_eq!(&reply[12..ask.len()], &ask[12..]);
        assert_eq!(
            &reply[len - 4..len],
            &[192, 168, 4, 1],
            "the record's data is the address"
        );
        assert_eq!(
            u16::from_be_bytes([reply[2], reply[3]]) & 0x8000,
            0x8000,
            "QR"
        );
    }

    #[test]
    fn a_query_for_something_else_is_answered_with_nothing() {
        // A client waiting on an AAAA answer is a client that must be answered, or
        // it sits on a timeout instead of falling back to IPv4.
        let ask = query(&[b"example"], 28);
        let mut reply = [0u8; 128];
        let len = answer(&ask, [192, 168, 4, 1], &mut reply).expect("room enough");
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 0, "no answers");
        assert_eq!(len, ask.len(), "the question and nothing else");
    }

    #[test]
    fn a_buffer_that_cannot_hold_the_answer_is_refused_rather_than_truncated() {
        let ask = query(&[b"example"], TYPE_A);
        let mut reply = [0u8; 24];
        assert_eq!(answer(&ask, [192, 168, 4, 1], &mut reply), None);
    }

    #[test]
    fn a_truncated_question_is_refused() {
        let ask = query(&[b"example"], TYPE_A);
        for cut in [0, 5, 11, 12, 14, ask.len() - 1] {
            let mut reply = [0u8; 128];
            assert_eq!(
                answer(&ask[..cut], [192, 168, 4, 1], &mut reply),
                None,
                "a query cut at {cut} is not one"
            );
        }
    }

    #[test]
    fn a_label_that_claims_to_be_a_pointer_is_refused() {
        // A query's name is uncompressed, so this is a malformed one. Following the
        // pointer would be reading an offset the client never wrote.
        let mut ask = query(&[b"example"], TYPE_A);
        ask[12] = 0xc0;
        ask[13] = 0x00;
        let mut reply = [0u8; 128];
        assert_eq!(answer(&ask, [192, 168, 4, 1], &mut reply), None);
    }

    #[test]
    fn a_query_with_no_question_is_refused() {
        let ask = [0u8; 12];
        let mut reply = [0u8; 128];
        assert_eq!(answer(&ask, [192, 168, 4, 1], &mut reply), None);
    }
}
