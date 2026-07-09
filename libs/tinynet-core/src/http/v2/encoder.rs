use crate::http::Request;
use crate::http::v2::frame;
use crate::http::v2::hpack;

pub struct Encoder {
    next_stream_id: u32,
}

impl Encoder {
    pub fn new() -> Self {
        Encoder { next_stream_id: 1 }
    }

    pub fn after_upgrade() -> Self {
        Encoder { next_stream_id: 3 }
    }

    pub fn encode(&mut self, req: &Request<'_>) -> Vec<u8> {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2;

        let headers = hpack::encode_request(req);
        let mut out = Vec::new();

        write_header_block(&mut out, stream_id, &headers, req.body.is_none());
        if let Some(body) = &req.body {
            frame::write_chunked(
                &mut out,
                frame::DATA,
                frame::FLAG_END_STREAM,
                stream_id,
                body.as_ref(),
            );
        }

        out
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

fn write_header_block(out: &mut Vec<u8>, stream_id: u32, headers: &[u8], end_stream: bool) {
    let first_len = headers.len().min(frame::DEFAULT_MAX_FRAME_SIZE);
    let flags = if first_len == headers.len() {
        frame::FLAG_END_HEADERS
    } else {
        0
    } | if end_stream {
        frame::FLAG_END_STREAM
    } else {
        0
    };

    frame::write(out, frame::HEADERS, flags, stream_id, &headers[..first_len]);

    if first_len < headers.len() {
        frame::write_chunked(
            out,
            frame::CONTINUATION,
            frame::FLAG_END_HEADERS,
            stream_id,
            &headers[first_len..],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::v2::frame::{self, parse_header};
    use crate::http::{Method, Request};

    #[test]
    fn get_produces_single_headers_frame() {
        let req = Request::try_from("http://example.com/").unwrap();
        let frames = Encoder::new().encode(&req);

        let (payload_len, ftype, flags, sid) = parse_header(&frames);
        assert_eq!(ftype, frame::HEADERS);
        assert_eq!(flags, frame::FLAG_END_HEADERS | frame::FLAG_END_STREAM);
        assert_eq!(sid, 1);
        assert_eq!(frames.len(), frame::HEADER_LEN + payload_len);
    }

    #[test]
    fn post_with_body_produces_headers_then_data() {
        let mut req = Request::try_from("https://example.com/api").unwrap();
        req.method = Method::POST;
        let req = req.body("hello");

        let frames = Encoder::new().encode(&req);

        let (hlen, htype, hflags, hsid) = parse_header(&frames);
        assert_eq!(htype, frame::HEADERS);
        assert_eq!(hflags, frame::FLAG_END_HEADERS);
        assert_eq!(hsid, 1);

        let data_start = frame::HEADER_LEN + hlen;
        let (dlen, dtype, dflags, dsid) = parse_header(&frames[data_start..]);
        assert_eq!(dtype, frame::DATA);
        assert_eq!(dflags, frame::FLAG_END_STREAM);
        assert_eq!(dsid, 1);
        assert_eq!(&frames[data_start + frame::HEADER_LEN..][..dlen], b"hello");
    }

    #[test]
    fn stream_ids_are_odd_and_increment_by_two() {
        let req = Request::try_from("http://example.com/").unwrap();
        let mut enc = Encoder::new();
        let ids: Vec<u32> = (0..3).map(|_| parse_header(&enc.encode(&req)).3).collect();
        assert_eq!(ids, [1, 3, 5]);
    }

    #[test]
    fn large_header_block_uses_continuation_frames() {
        let req = Request::try_from("http://example.com/")
            .unwrap()
            .header("x-large", "a".repeat(frame::DEFAULT_MAX_FRAME_SIZE));
        let frames = Encoder::new().encode(&req);

        let (hlen, htype, hflags, hsid) = parse_header(&frames);
        assert_eq!(htype, frame::HEADERS);
        assert_eq!(hflags, frame::FLAG_END_STREAM);
        assert_eq!(hsid, 1);
        assert_eq!(hlen, frame::DEFAULT_MAX_FRAME_SIZE);

        let continuation = frame::HEADER_LEN + hlen;
        let (_, ctype, cflags, csid) = parse_header(&frames[continuation..]);
        assert_eq!(ctype, frame::CONTINUATION);
        assert_eq!(cflags, frame::FLAG_END_HEADERS);
        assert_eq!(csid, 1);
    }

    #[test]
    fn large_body_is_split_into_data_frames() {
        let mut req = Request::try_from("http://example.com/").unwrap();
        req.method = Method::POST;
        let req = req.body(vec![b'x'; frame::DEFAULT_MAX_FRAME_SIZE + 1]);
        let frames = Encoder::new().encode(&req);

        let (hlen, _, _, _) = parse_header(&frames);
        let first_data = frame::HEADER_LEN + hlen;
        let (dlen, dtype, dflags, _) = parse_header(&frames[first_data..]);
        assert_eq!(dlen, frame::DEFAULT_MAX_FRAME_SIZE);
        assert_eq!(dtype, frame::DATA);
        assert_eq!(dflags, 0);

        let second_data = first_data + frame::HEADER_LEN + dlen;
        let (last_len, last_type, last_flags, _) = parse_header(&frames[second_data..]);
        assert_eq!(last_len, 1);
        assert_eq!(last_type, frame::DATA);
        assert_eq!(last_flags, frame::FLAG_END_STREAM);
    }
}
