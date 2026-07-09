use crate::buf::WriteBuf;
use crate::http::{Bytes, Method, Request, Response};
use crate::utils::{StrLike, find_nth};

pub fn encode_request(req: &Request<'_>) -> Vec<u8> {
    let mut buffer = WriteBuf::new();
    encode_request_into(req, &mut buffer);
    buffer.concat()
}

pub fn encode_request_into(req: &Request<'_>, buffer: &mut WriteBuf) {
    buffer.put(method_bytes(&req.method));
    buffer.put_static(b" ");
    put_sanitized_http_field(buffer, req.target.as_ref());
    buffer.put_static(b" HTTP/1.1\r\nHost: ");
    put_sanitized_http_field(buffer, req.host.as_ref());
    buffer.put_static(b"\r\n");

    for (name, value) in &req.headers {
        put_sanitized_http_field(buffer, name.as_ref());
        buffer.put_static(b": ");
        put_sanitized_http_field(buffer, value.as_ref());
        buffer.put_static(b"\r\n");
    }

    if let Some(body) = &req.body {
        let body = body.as_ref();
        if !has_header(&req.headers, "content-length") {
            buffer.put_static(b"Content-Length: ");
            buffer.put(body.len().to_string().as_bytes());
            buffer.put_static(b"\r\n");
        }
        buffer.put_static(b"\r\n");
        buffer.put(body);
    } else {
        buffer.put_static(b"\r\n");
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

fn extend_sanitized_http_field(buf: &mut Vec<u8>, value: &str) {
    buf.extend(value.bytes().filter(|b| *b != b'\r' && *b != b'\n'));
}

fn put_sanitized_http_field(buffer: &mut WriteBuf, value: &str) {
    for part in value.split(['\r', '\n']) {
        buffer.put(part.as_bytes());
    }
}

pub fn response_length(buf: &[u8]) -> Option<usize> {
    message_length(buf)
}

fn find_content_length(headers: &[u8]) -> Option<usize> {
    let s = core::str::from_utf8(headers).ok()?;
    for line in s.split("\r\n").skip(1) {
        let Some(colon) = find_nth(line, ':', 1) else {
            continue;
        };
        if line[..colon].trim().eq_ignore_ascii_case("content-length") {
            return line[colon + 1..].trim().parse().ok();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Response decoder
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum DecodeError {
    InvalidRequestLine,
    InvalidStatusLine,
    InvalidHeader,
    InvalidContentLength,
    NonUtf8,
    Truncated,
}

pub fn decode_response(buf: &[u8]) -> Result<Response<'_>, DecodeError> {
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(DecodeError::Truncated)?;

    let header_section = core::str::from_utf8(&buf[..sep]).map_err(|_| DecodeError::NonUtf8)?;
    let body_offset = sep + 4;

    let mut lines = header_section.split("\r\n");

    let status_line = lines.next().ok_or(DecodeError::InvalidStatusLine)?;
    let (status, reason) = parse_status_line(status_line)?;

    let mut headers: Vec<(StrLike<'_>, StrLike<'_>)> = Vec::new();
    let mut content_length: Option<usize> = None;

    for line in lines {
        let colon = find_nth(line, ':', 1).ok_or(DecodeError::InvalidHeader)?;
        let name = line[..colon].trim();
        let value = line[colon + 1..].trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .parse()
                    .map_err(|_| DecodeError::InvalidContentLength)?,
            );
        }
        headers.push((StrLike::Borrowed(name), StrLike::Borrowed(value)));
    }

    let body = body(buf, body_offset, content_length)?;

    Ok(Response {
        status,
        reason: StrLike::Borrowed(reason),
        headers,
        body,
    })
}

pub fn encode_response(resp: &Response<'_>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    buf.extend_from_slice(b"HTTP/1.1 ");
    buf.extend_from_slice(resp.status.to_string().as_bytes());
    if !resp.reason.as_ref().is_empty() {
        buf.push(b' ');
        extend_sanitized_http_field(&mut buf, resp.reason.as_ref());
    }
    buf.extend_from_slice(b"\r\n");

    for (name, value) in &resp.headers {
        extend_sanitized_http_field(&mut buf, name.as_ref());
        buf.extend_from_slice(b": ");
        extend_sanitized_http_field(&mut buf, value.as_ref());
        buf.extend_from_slice(b"\r\n");
    }

    if let Some(body) = &resp.body {
        if !has_header(&resp.headers, "content-length") {
            buf.extend_from_slice(b"Content-Length: ");
            buf.extend_from_slice(body.as_ref().len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        buf.extend_from_slice(b"\r\n");
        buf.extend_from_slice(body.as_ref());
    } else {
        buf.extend_from_slice(b"\r\n");
    }

    buf
}

pub fn request_length(buf: &[u8]) -> Option<usize> {
    message_length(buf)
}

pub fn decode_request(buf: &[u8]) -> Result<Request<'_>, DecodeError> {
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(DecodeError::Truncated)?;

    let header_section = core::str::from_utf8(&buf[..sep]).map_err(|_| DecodeError::NonUtf8)?;
    let body_offset = sep + 4;

    let mut lines = header_section.split("\r\n");
    let request_line = lines.next().ok_or(DecodeError::InvalidRequestLine)?;
    let (method, target) = parse_request_line(request_line)?;

    let mut host = StrLike::Str("");
    let mut headers: Vec<(StrLike<'_>, StrLike<'_>)> = Vec::new();
    let mut content_length: Option<usize> = None;

    for line in lines {
        let colon = find_nth(line, ':', 1).ok_or(DecodeError::InvalidHeader)?;
        let name = line[..colon].trim();
        let value = line[colon + 1..].trim();
        if name.eq_ignore_ascii_case("host") {
            host = StrLike::Borrowed(value);
            continue;
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .parse()
                    .map_err(|_| DecodeError::InvalidContentLength)?,
            );
            continue;
        }
        headers.push((StrLike::Borrowed(name), StrLike::Borrowed(value)));
    }

    let body = body(buf, body_offset, content_length)?;

    Ok(Request {
        method,
        scheme: "http".into(),
        host,
        target,
        headers,
        body,
    })
}

pub fn encode(req: &Request<'_>) -> Vec<u8> {
    encode_request(req)
}

pub fn decode(buf: &[u8]) -> Result<Response<'_>, DecodeError> {
    decode_response(buf)
}

fn message_length(buf: &[u8]) -> Option<usize> {
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let body_len = find_content_length(&buf[..sep]).unwrap_or(0);
    let total = sep.checked_add(4)?.checked_add(body_len)?;
    if buf.len() >= total {
        Some(total)
    } else {
        None
    }
}

fn has_header(headers: &[(StrLike<'_>, StrLike<'_>)], needle: &str) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.as_ref().eq_ignore_ascii_case(needle))
}

fn body(
    buf: &[u8],
    body_offset: usize,
    content_length: Option<usize>,
) -> Result<Option<Bytes<'_>>, DecodeError> {
    let Some(len) = content_length else {
        return Ok(None);
    };
    let body_end = body_offset
        .checked_add(len)
        .ok_or(DecodeError::InvalidContentLength)?;
    if body_end > buf.len() {
        return Err(DecodeError::Truncated);
    }
    Ok(Some(Bytes::Borrowed(&buf[body_offset..body_end])))
}

fn parse_request_line(line: &str) -> Result<(Method<'_>, StrLike<'_>), DecodeError> {
    let sp1 = find_nth(line, ' ', 1).ok_or(DecodeError::InvalidRequestLine)?;
    let rest = &line[sp1 + 1..];
    let sp2 = find_nth(rest, ' ', 1).ok_or(DecodeError::InvalidRequestLine)?;
    let version = &rest[sp2 + 1..];
    if !version.starts_with("HTTP/") {
        return Err(DecodeError::InvalidRequestLine);
    }
    Ok((parse_method(&line[..sp1]), StrLike::Borrowed(&rest[..sp2])))
}

fn parse_method(method: &str) -> Method<'_> {
    match method {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        "PATCH" => Method::PATCH,
        "HEAD" => Method::HEAD,
        "OPTIONS" => Method::OPTIONS,
        "CONNECT" => Method::CONNECT,
        "TRACE" => Method::TRACE,
        _ => Method::Custom(StrLike::Borrowed(method)),
    }
}

fn parse_status_line(line: &str) -> Result<(u16, &str), DecodeError> {
    // Expected: HTTP/1.x 200 OK  (reason phrase is optional)
    if !line.starts_with("HTTP/") {
        return Err(DecodeError::InvalidStatusLine);
    }
    let sp1 = find_nth(line, ' ', 1).ok_or(DecodeError::InvalidStatusLine)?;
    let rest = &line[sp1 + 1..];
    let (code_str, reason) = if let Some(sp2) = find_nth(rest, ' ', 1) {
        (&rest[..sp2], &rest[sp2 + 1..])
    } else {
        (rest, "")
    };
    let status: u16 = code_str
        .parse()
        .map_err(|_| DecodeError::InvalidStatusLine)?;
    Ok((status, reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Request;
    use crate::tcp::{TcpListener, TcpStream};

    // --- encode ---

    #[test]
    fn get_no_body() {
        let req = Request::try_from("https://example.com/api/data?x=1").unwrap();
        assert_eq!(
            encode(&req),
            b"GET /api/data?x=1 HTTP/1.1\r\nHost: example.com\r\n\r\n"
        );
    }

    #[test]
    fn get_no_path() {
        let req = Request::try_from("https://example.com").unwrap();
        assert_eq!(encode(&req), b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n");
    }

    #[test]
    fn post_with_headers_and_body() {
        let mut req = Request::try_from("https://example.com/api").unwrap();
        req.method = Method::POST;
        let req = req
            .header("Content-Type", "application/json")
            .body("{\"x\":1}");
        assert_eq!(
            encode(&req),
            b"POST /api HTTP/1.1\r\nHost: example.com\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"x\":1}"
        );
    }

    #[test]
    fn encode_into_matches_encode_request() {
        let request = Request {
            method: Method::POST,
            scheme: "http".into(),
            host: "example.com".into(),
            target: "/upload".into(),
            headers: vec![("X-Test".into(), "yes".into())],
            body: Some(Bytes::Borrowed(b"body")),
        };
        let expected = encode_request(&request);
        let mut buffer = WriteBuf::new();
        encode_request_into(&request, &mut buffer);
        assert_eq!(buffer.concat(), expected);
    }

    // --- decode ---

    #[test]
    fn encode_response_with_body() {
        let resp = Response {
            status: 200,
            reason: "OK".into(),
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: Some("hello".into()),
        };

        assert_eq!(
            encode_response(&resp),
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello"
        );
    }

    #[test]
    fn decode_request_with_body() {
        let raw = b"POST /echo HTTP/1.1\r\nHost: example.com\r\nContent-Type: text/plain\r\nContent-Length: 4\r\n\r\nping";
        let req = decode_request(raw).unwrap();

        assert!(matches!(req.method, Method::POST));
        assert_eq!(req.scheme.as_ref(), "http");
        assert_eq!(req.host.as_ref(), "example.com");
        assert_eq!(req.target.as_ref(), "/echo");
        assert_eq!(req.headers[0].0.as_ref(), "Content-Type");
        assert_eq!(req.headers[0].1.as_ref(), "text/plain");
        assert_eq!(req.body.unwrap().as_ref(), b"ping");
    }

    #[test]
    fn request_length_waits_for_complete_body() {
        let raw = b"POST / HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\n\r\npi";
        assert_eq!(request_length(raw), None);

        let raw = b"POST / HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\n\r\npingextra";
        assert_eq!(request_length(raw), Some(raw.len() - 5));
    }

    #[test]
    fn decode_200_with_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let resp = decode(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.reason.as_ref(), "OK");
        assert_eq!(resp.body.unwrap().as_ref(), b"hello");
    }

    #[test]
    fn decode_response_borrows_from_input() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello";
        let resp = decode_response(raw).unwrap();

        assert!(matches!(resp.reason, StrLike::Borrowed("OK")));
        assert!(matches!(
            resp.headers[0].0,
            StrLike::Borrowed("Content-Type")
        ));
        assert!(matches!(resp.headers[0].1, StrLike::Borrowed("text/plain")));
        assert!(matches!(resp.body, Some(Bytes::Borrowed(b"hello"))));
    }

    #[test]
    fn decode_request_borrows_from_input() {
        let raw = b"POST /echo HTTP/1.1\r\nHost: example.com\r\nContent-Type: text/plain\r\nContent-Length: 4\r\n\r\nping";
        let req = decode_request(raw).unwrap();

        assert!(matches!(req.host, StrLike::Borrowed("example.com")));
        assert!(matches!(req.target, StrLike::Borrowed("/echo")));
        assert!(matches!(
            req.headers[0].0,
            StrLike::Borrowed("Content-Type")
        ));
        assert!(matches!(req.headers[0].1, StrLike::Borrowed("text/plain")));
        assert!(matches!(req.body, Some(Bytes::Borrowed(b"ping"))));
    }

    #[test]
    fn decode_no_body() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let resp = decode(raw).unwrap();
        assert_eq!(resp.status, 204);
        assert_eq!(resp.reason.as_ref(), "No Content");
        assert!(resp.body.is_none());
    }

    #[test]
    fn decode_headers_preserved() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\nabc";
        let resp = decode(raw).unwrap();
        assert_eq!(resp.headers[0].0.as_ref(), "Content-Type");
        assert_eq!(resp.headers[0].1.as_ref(), "text/plain");
        assert_eq!(resp.body.unwrap().as_ref(), b"abc");
    }

    #[test]
    fn decode_no_reason_phrase() {
        let raw = b"HTTP/1.1 200\r\n\r\n";
        let resp = decode(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.reason.as_ref(), "");
    }

    #[test]
    fn decode_truncated_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort";
        assert!(matches!(decode(raw), Err(DecodeError::Truncated)));
    }

    #[test]
    fn decode_rejects_content_length_overflow() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\nx";
        assert!(matches!(
            decode(raw),
            Err(DecodeError::InvalidContentLength)
        ));
    }

    #[test]
    fn response_length_handles_content_length_overflow() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\nx";
        assert_eq!(response_length(raw), None);
    }

    #[test]
    fn encode_strips_crlf_from_header_values() {
        let req = Request::try_from("https://example.com/")
            .unwrap()
            .header("X-Test", "ok\r\nInjected: yes");
        let encoded = encode(&req);
        let encoded = core::str::from_utf8(&encoded).unwrap();
        assert!(!encoded.contains("\r\nInjected: yes"));
        assert!(encoded.contains("X-Test: okInjected: yes\r\n"));
    }

    #[test]
    fn encode_strips_crlf_from_host_and_target() {
        let req = Request {
            method: Method::GET,
            scheme: "https".into(),
            host: "example.com\r\nInjected: yes".to_string().into(),
            target: "/safe\r\nX: y".to_string().into(),
            headers: Vec::new(),
            body: None,
        };
        let encoded = encode(&req);
        let encoded = core::str::from_utf8(&encoded).unwrap();
        assert!(!encoded.contains("\r\nInjected: yes"));
        assert!(
            encoded.starts_with("GET /safeX: y HTTP/1.1\r\nHost: example.comInjected: yes\r\n")
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn http1_client_server() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();

            // Drain the incoming request (GET, no body — ends at \r\n\r\n)
            let mut req_buf = [0u8; 512];
            conn.read(&mut req_buf).await.unwrap();

            conn.write(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nworld",
            )
            .await
            .unwrap();
        });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let req = Request::try_from("http://example.com/hello").unwrap();
        stream.write(&encode(&req)).await.unwrap();

        let mut resp_buf = [0u8; 512];
        let n = stream.read(&mut resp_buf).await.unwrap();
        let resp = decode(&resp_buf[..n]).unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.headers[0].0.as_ref(), "Content-Type");
        assert_eq!(resp.headers[0].1.as_ref(), "text/plain");
        assert_eq!(resp.body.unwrap().as_ref(), b"world");

        server.await.unwrap();
    }
}
