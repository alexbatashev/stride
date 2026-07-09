use crate::http::{Method, Request};

const STATIC_AUTHORITY: usize = 1;
const STATIC_METHOD_GET: usize = 2;
const STATIC_METHOD_POST: usize = 3;
const STATIC_PATH_ROOT: usize = 4;
const STATIC_SCHEME_HTTP: usize = 6;
const STATIC_SCHEME_HTTPS: usize = 7;
const STATIC_CONTENT_LENGTH: usize = 28;

pub(crate) fn encode_request(req: &Request<'_>) -> Vec<u8> {
    let mut buf = Vec::new();

    encode_method(&req.method, &mut buf);
    encode_scheme(req.scheme.as_ref(), &mut buf);
    encode_path(req.target.as_ref(), &mut buf);
    literal_indexed_name(STATIC_AUTHORITY, req.host.as_ref().as_bytes(), &mut buf);

    let mut has_content_length = false;
    for (name, value) in &req.headers {
        has_content_length |= name.as_ref().eq_ignore_ascii_case("content-length");
        literal_new_name_lowercase(name.as_ref(), value.as_ref().as_bytes(), &mut buf);
    }

    if let Some(body) = &req.body
        && !has_content_length
    {
        let len = body.as_ref().len().to_string();
        literal_indexed_name(STATIC_CONTENT_LENGTH, len.as_bytes(), &mut buf);
    }

    buf
}

fn encode_method(method: &Method, buf: &mut Vec<u8>) {
    match method {
        Method::GET => indexed(STATIC_METHOD_GET, buf),
        Method::POST => indexed(STATIC_METHOD_POST, buf),
        method => literal_new_name(b":method", method_bytes(method), buf),
    }
}

fn encode_scheme(scheme: &str, buf: &mut Vec<u8>) {
    match scheme {
        "http" => indexed(STATIC_SCHEME_HTTP, buf),
        "https" => indexed(STATIC_SCHEME_HTTPS, buf),
        scheme => literal_new_name(b":scheme", scheme.as_bytes(), buf),
    }
}

fn encode_path(path: &str, buf: &mut Vec<u8>) {
    if path == "/" {
        indexed(STATIC_PATH_ROOT, buf);
    } else {
        literal_indexed_name(STATIC_PATH_ROOT, path.as_bytes(), buf);
    }
}

fn method_bytes<'a>(method: &'a Method<'a>) -> &'a [u8] {
    match method {
        Method::GET => b"GET",
        Method::POST => b"POST",
        Method::PUT => b"PUT",
        Method::DELETE => b"DELETE",
        Method::PATCH => b"PATCH",
        Method::HEAD => b"HEAD",
        Method::OPTIONS => b"OPTIONS",
        Method::CONNECT => b"CONNECT",
        Method::TRACE => b"TRACE",
        Method::Custom(s) => s.as_ref().as_bytes(),
    }
}

fn indexed(index: usize, buf: &mut Vec<u8>) {
    encode_int(index, 7, 0x80, buf);
}

fn literal_indexed_name(name_idx: usize, value: &[u8], buf: &mut Vec<u8>) {
    encode_int(name_idx, 4, 0x00, buf);
    encode_string(value, buf);
}

fn literal_new_name(name: &[u8], value: &[u8], buf: &mut Vec<u8>) {
    buf.push(0x00);
    encode_string(name, buf);
    encode_string(value, buf);
}

fn literal_new_name_lowercase(name: &str, value: &[u8], buf: &mut Vec<u8>) {
    let name = name.to_ascii_lowercase();
    literal_new_name(name.as_bytes(), value, buf);
}

fn encode_int(value: usize, prefix_bits: u8, high_bits: u8, buf: &mut Vec<u8>) {
    let max = (1usize << prefix_bits) - 1;
    if value < max {
        buf.push(high_bits | value as u8);
    } else {
        buf.push(high_bits | max as u8);
        let mut n = value - max;
        while n >= 128 {
            buf.push((n & 0x7F | 0x80) as u8);
            n >>= 7;
        }
        buf.push(n as u8);
    }
}

fn encode_string(s: &[u8], buf: &mut Vec<u8>) {
    encode_int(s.len(), 7, 0x00, buf);
    buf.extend_from_slice(s);
}

pub(crate) fn decode_headers(mut buf: &[u8]) -> std::io::Result<Vec<(String, String)>> {
    let mut out = Vec::new();

    while !buf.is_empty() {
        let b = buf[0];
        if b & 0x80 != 0 {
            let index = decode_int(&mut buf, 7)?;
            let (name, value) = static_header(index).ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported HTTP/2 header index",
            ))?;
            out.push((name.to_owned(), value.to_owned()));
        } else if b & 0x40 != 0 {
            let index = decode_int(&mut buf, 6)?;
            let name = decode_name(index, &mut buf)?;
            let value = decode_string(&mut buf)?;
            out.push((name, value));
        } else if b & 0x20 != 0 {
            let _ = decode_int(&mut buf, 5)?;
        } else {
            let index = decode_int(&mut buf, 4)?;
            let name = decode_name(index, &mut buf)?;
            let value = decode_string(&mut buf)?;
            out.push((name, value));
        }
    }

    Ok(out)
}

fn decode_name(index: usize, buf: &mut &[u8]) -> std::io::Result<String> {
    if index == 0 {
        decode_string(buf)
    } else {
        let (name, _) = static_header(index).ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported HTTP/2 header name index",
        ))?;
        Ok(name.to_owned())
    }
}

fn decode_int(buf: &mut &[u8], prefix_bits: u8) -> std::io::Result<usize> {
    if buf.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "truncated HTTP/2 integer",
        ));
    }
    let mask = (1u8 << prefix_bits) - 1;
    let mut value = (buf[0] & mask) as usize;
    *buf = &buf[1..];
    if value < mask as usize {
        return Ok(value);
    }

    let mut shift = 0;
    loop {
        if buf.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated HTTP/2 integer",
            ));
        }
        let b = buf[0];
        *buf = &buf[1..];
        value += ((b & 0x7f) as usize) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn decode_string(buf: &mut &[u8]) -> std::io::Result<String> {
    if buf.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "truncated HTTP/2 string",
        ));
    }
    let huffman = buf[0] & 0x80 != 0;
    let len = decode_int(buf, 7)?;
    if huffman {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "HTTP/2 huffman strings are unsupported",
        ));
    }
    if buf.len() < len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "truncated HTTP/2 string",
        ));
    }
    let s = core::str::from_utf8(&buf[..len])
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        .to_owned();
    *buf = &buf[len..];
    Ok(s)
}

fn static_header(index: usize) -> Option<(&'static str, &'static str)> {
    Some(match index {
        1 => (":authority", ""),
        2 => (":method", "GET"),
        3 => (":method", "POST"),
        4 => (":path", "/"),
        5 => (":path", "/index.html"),
        6 => (":scheme", "http"),
        7 => (":scheme", "https"),
        8 => (":status", "200"),
        9 => (":status", "204"),
        10 => (":status", "301"),
        11 => (":status", "302"),
        12 => (":status", "304"),
        13 => (":status", "400"),
        14 => (":status", "404"),
        15 => (":status", "500"),
        16 => ("accept-charset", ""),
        17 => ("accept-encoding", "gzip, deflate"),
        18 => ("accept-language", ""),
        19 => ("accept-ranges", ""),
        20 => ("access-control-allow-origin", ""),
        21 => ("age", ""),
        22 => ("authorization", ""),
        23 => ("cache-control", ""),
        24 => ("connection", ""),
        25 => ("content-disposition", ""),
        26 => ("content-encoding", ""),
        27 => ("content-language", ""),
        28 => ("content-length", ""),
        29 => ("content-location", ""),
        30 => ("content-range", ""),
        31 => ("content-type", ""),
        32 => ("cookie", ""),
        33 => ("date", ""),
        34 => ("expect", ""),
        35 => ("forwarded", ""),
        36 => ("host", ""),
        37 => ("if-match", ""),
        38 => ("if-modified-since", ""),
        39 => ("if-none-match", ""),
        40 => ("if-range", ""),
        41 => ("if-unmodified-since", ""),
        42 => ("last-modified", ""),
        43 => ("link", ""),
        44 => ("location", ""),
        45 => ("max-forwards", ""),
        46 => ("origin", ""),
        47 => ("priority", ""),
        48 => ("referer", ""),
        49 => ("retry-after", ""),
        50 => ("server", ""),
        51 => ("set-cookie", ""),
        52 => ("te", ""),
        53 => ("trailer", ""),
        54 => ("transfer-encoding", ""),
        55 => ("upgrade", ""),
        56 => ("user-agent", ""),
        57 => ("vary", ""),
        58 => ("via", ""),
        59 => ("www-authenticate", ""),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Request;

    #[test]
    fn encodes_get_http_root_with_static_entries() {
        let req = Request::try_from("http://example.com/").unwrap();
        let mut expected = vec![0x82u8, 0x86, 0x84, 0x01, 0x0B];
        expected.extend_from_slice(b"example.com");
        assert_eq!(encode_request(&req), expected);
    }

    #[test]
    fn lowercases_regular_header_names() {
        let req = Request::try_from("http://example.com/")
            .unwrap()
            .header("X-Trace", "abc");

        let hblock = encode_request(&req);

        assert!(hblock.windows(b"x-trace".len()).any(|w| w == b"x-trace"));
        assert!(!hblock.windows(b"X-Trace".len()).any(|w| w == b"X-Trace"));
    }

    #[test]
    fn does_not_duplicate_existing_content_length() {
        let mut req = Request::try_from("http://example.com/").unwrap();
        req.method = Method::POST;
        let req = req.header("Content-Length", "5").body("hello");
        let hblock = encode_request(&req);
        let content_length_fields = hblock
            .windows(b"content-length".len())
            .filter(|w| *w == b"content-length")
            .count();

        assert_eq!(content_length_fields, 1);
    }

    #[test]
    fn content_length_index_uses_multibyte_int() {
        let mut req = Request::try_from("http://example.com/").unwrap();
        req.method = Method::POST;
        let req = req.body("hello");
        let hblock = encode_request(&req);
        let cl_pos = hblock.windows(2).position(|w| w == [0x0F, 0x0D]).unwrap();
        assert_eq!(&hblock[cl_pos + 2..cl_pos + 4], &[0x01, b'5']);
    }
}
