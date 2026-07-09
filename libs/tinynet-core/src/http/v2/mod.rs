mod encoder;
mod frame;
mod hpack;

pub use encoder::Encoder;

pub const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
pub const SETTINGS_FRAME: [u8; frame::HEADER_LEN] = [0, 0, 0, frame::SETTINGS, 0, 0, 0, 0, 0];

use std::io;

use crate::http::{Bytes, Response};
use crate::utils::StrLike;

pub fn decode_response(buf: &[u8]) -> io::Result<Response<'static>> {
    let mut pos = 0;
    let mut status = None;
    let mut headers = Vec::new();
    let mut body = Vec::new();

    while pos + frame::HEADER_LEN <= buf.len() {
        let (len, frame_type, flags, _) = frame::parse_header(&buf[pos..]);
        let payload_start = pos + frame::HEADER_LEN;
        let payload_end = payload_start + len;
        if payload_end > buf.len() {
            break;
        }
        let payload = &buf[payload_start..payload_end];

        match frame_type {
            frame::HEADERS => {
                let decoded = hpack::decode_headers(payload)?;
                for (name, value) in decoded {
                    if name == ":status" {
                        status = Some(value.parse().map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP/2 status")
                        })?);
                    } else {
                        headers.push((StrLike::Owned(name), StrLike::Owned(value)));
                    }
                }
                if flags & frame::FLAG_END_STREAM != 0 {
                    return response(status, headers, body);
                }
            }
            frame::DATA => {
                body.extend_from_slice(payload);
                if flags & frame::FLAG_END_STREAM != 0 {
                    return response(status, headers, body);
                }
            }
            _ => {}
        }

        pos = payload_end;
    }

    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "truncated HTTP/2 response",
    ))
}

pub fn response_length(buf: &[u8]) -> Option<usize> {
    let mut pos = 0;
    while pos + frame::HEADER_LEN <= buf.len() {
        let (len, frame_type, flags, _) = frame::parse_header(&buf[pos..]);
        let end = pos + frame::HEADER_LEN + len;
        if end > buf.len() {
            return None;
        }
        if matches!(frame_type, frame::HEADERS | frame::DATA) && flags & frame::FLAG_END_STREAM != 0
        {
            return Some(end);
        }
        pos = end;
    }
    None
}

fn response(
    status: Option<u16>,
    headers: Vec<(StrLike<'static>, StrLike<'static>)>,
    body: Vec<u8>,
) -> io::Result<Response<'static>> {
    Ok(Response {
        status: status.ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing HTTP/2 status",
        ))?,
        reason: "".into(),
        headers,
        body: if body.is_empty() {
            None
        } else {
            Some(Bytes::Owned(body))
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_frame_is_well_formed() {
        assert_eq!(SETTINGS_FRAME[0..3], [0, 0, 0]);
        assert_eq!(SETTINGS_FRAME[3], frame::SETTINGS);
        assert_eq!(SETTINGS_FRAME[4], 0);
        assert_eq!(SETTINGS_FRAME[5..9], [0, 0, 0, 0]);
    }
}
