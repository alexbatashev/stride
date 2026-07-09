use std::{collections::VecDeque, ops::Range};

use crate::{buf::IoBuf, utils::find_byte};

#[derive(Clone, Copy)]
pub struct H1Limits {
    pub max_head_bytes: usize,
    pub max_headers: usize,
    pub max_chunk_ext: usize,
}

impl Default for H1Limits {
    fn default() -> Self {
        Self {
            max_head_bytes: 64 * 1024,
            max_headers: 100,
            max_chunk_ext: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H1Error {
    HeadTooLarge,
    TooManyHeaders,
    InvalidStatusLine,
    InvalidHeader,
    InvalidContentLength,
    InvalidTransferEncoding,
    InvalidChunk,
    ChunkExtensionTooLarge,
    UnexpectedEof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFraming {
    ContentLength(u64),
    Chunked,
    UntilEof,
    None,
}

pub struct ResponseHead {
    pub status: u16,
    pub reason: IoBuf,
    pub headers: Vec<(IoBuf, IoBuf)>,
}

pub enum H1Event {
    Head(ResponseHead),
    Body(IoBuf),
    End {
        keep_alive: bool,
        trailers: Vec<(IoBuf, IoBuf)>,
    },
}

#[derive(Default)]
pub struct RecvBuf {
    chunks: VecDeque<IoBuf>,
    len: usize,
    eof: bool,
}

impl RecvBuf {
    pub fn push(&mut self, chunk: IoBuf) {
        if !chunk.is_empty() {
            self.len += chunk.len();
            self.chunks.push_back(chunk);
        }
    }

    pub fn set_eof(&mut self) {
        self.eof = true;
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn copy_prefix(&self, limit: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.len.min(limit));
        for chunk in &self.chunks {
            let remaining = limit - output.len();
            if remaining == 0 {
                break;
            }
            output.extend_from_slice(&chunk.as_ref()[..chunk.len().min(remaining)]);
        }
        output
    }

    fn consume(&mut self, mut count: usize) {
        assert!(count <= self.len);
        self.len -= count;
        while count > 0 {
            let chunk = self.chunks.pop_front().unwrap();
            if count < chunk.len() {
                self.chunks.push_front(chunk.slice(count..chunk.len()));
                return;
            }
            count -= chunk.len();
        }
    }

    fn take_chunk(&mut self, limit: usize) -> Option<IoBuf> {
        let chunk = self.chunks.pop_front()?;
        let take = chunk.len().min(limit);
        self.len -= take;
        if take < chunk.len() {
            self.chunks.push_front(chunk.slice(take..chunk.len()));
        }
        Some(chunk.slice(0..take))
    }
}

#[derive(Clone, Copy)]
enum State {
    Head,
    Fixed { remaining: u64, keep_alive: bool },
    ChunkSize { keep_alive: bool },
    ChunkData { remaining: u64, keep_alive: bool },
    ChunkCrlf { keep_alive: bool },
    Trailers { keep_alive: bool },
    UntilEof,
    Done,
}

pub struct H1Parser {
    state: State,
    limits: H1Limits,
    response_to_head: bool,
    response_to_connect: bool,
}

impl Default for H1Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl H1Parser {
    pub fn new() -> Self {
        Self {
            state: State::Head,
            limits: H1Limits::default(),
            response_to_head: false,
            response_to_connect: false,
        }
    }

    pub fn with_limits(limits: H1Limits) -> Self {
        Self {
            limits,
            ..Self::new()
        }
    }

    pub fn response_to_head(mut self, value: bool) -> Self {
        self.response_to_head = value;
        self
    }

    pub fn response_to_connect(mut self, value: bool) -> Self {
        self.response_to_connect = value;
        self
    }

    pub fn decode(&mut self, input: &mut RecvBuf) -> Result<Option<H1Event>, H1Error> {
        match self.state {
            State::Head => self.decode_head(input),
            State::Fixed {
                remaining: 0,
                keep_alive,
            } => {
                self.state = State::Done;
                Ok(Some(H1Event::End {
                    keep_alive,
                    trailers: Vec::new(),
                }))
            }
            State::Fixed {
                remaining,
                keep_alive,
            } => {
                if let Some(chunk) = input.take_chunk(remaining as usize) {
                    self.state = State::Fixed {
                        remaining: remaining - chunk.len() as u64,
                        keep_alive,
                    };
                    return Ok(Some(H1Event::Body(chunk)));
                }
                if input.is_eof() {
                    return Err(H1Error::UnexpectedEof);
                }
                Ok(None)
            }
            State::ChunkSize { keep_alive } => {
                let Some(line) = take_line(input, self.limits.max_chunk_ext + 18)? else {
                    return Ok(None);
                };
                let (size, extension) = line
                    .iter()
                    .position(|byte| *byte == b';')
                    .map_or((line.as_slice(), &[][..]), |index| {
                        (&line[..index], &line[index + 1..])
                    });
                if extension.len() > self.limits.max_chunk_ext {
                    return Err(H1Error::ChunkExtensionTooLarge);
                }
                if size.is_empty() || size.len() > 16 || !size.iter().all(u8::is_ascii_hexdigit) {
                    return Err(H1Error::InvalidChunk);
                }
                let size = u64::from_str_radix(std::str::from_utf8(size).unwrap(), 16)
                    .map_err(|_| H1Error::InvalidChunk)?;
                self.state = if size == 0 {
                    State::Trailers { keep_alive }
                } else {
                    State::ChunkData {
                        remaining: size,
                        keep_alive,
                    }
                };
                self.decode(input)
            }
            State::ChunkData {
                remaining,
                keep_alive,
            } => {
                if let Some(chunk) = input.take_chunk(remaining as usize) {
                    let remaining = remaining - chunk.len() as u64;
                    self.state = if remaining == 0 {
                        State::ChunkCrlf { keep_alive }
                    } else {
                        State::ChunkData {
                            remaining,
                            keep_alive,
                        }
                    };
                    return Ok(Some(H1Event::Body(chunk)));
                }
                if input.is_eof() {
                    return Err(H1Error::UnexpectedEof);
                }
                Ok(None)
            }
            State::ChunkCrlf { keep_alive } => {
                if input.len() < 2 {
                    return if input.is_eof() {
                        Err(H1Error::UnexpectedEof)
                    } else {
                        Ok(None)
                    };
                }
                if input.copy_prefix(2) != b"\r\n" {
                    return Err(H1Error::InvalidChunk);
                }
                input.consume(2);
                self.state = State::ChunkSize { keep_alive };
                self.decode(input)
            }
            State::Trailers { keep_alive } => {
                if input.len() >= 2 && input.copy_prefix(2) == b"\r\n" {
                    input.consume(2);
                    self.state = State::Done;
                    return Ok(Some(H1Event::End {
                        keep_alive,
                        trailers: Vec::new(),
                    }));
                }
                let Some((block, consumed)) = take_head_block(input, self.limits.max_head_bytes)?
                else {
                    return Ok(None);
                };
                input.consume(consumed);
                let trailers = parse_trailers(block, self.limits.max_headers)?;
                self.state = State::Done;
                Ok(Some(H1Event::End {
                    keep_alive,
                    trailers,
                }))
            }
            State::UntilEof => {
                if let Some(chunk) = input.take_chunk(usize::MAX) {
                    return Ok(Some(H1Event::Body(chunk)));
                }
                if input.is_eof() {
                    self.state = State::Done;
                    return Ok(Some(H1Event::End {
                        keep_alive: false,
                        trailers: Vec::new(),
                    }));
                }
                Ok(None)
            }
            State::Done => Ok(None),
        }
    }

    fn decode_head(&mut self, input: &mut RecvBuf) -> Result<Option<H1Event>, H1Error> {
        let Some((block, consumed)) = take_head_block(input, self.limits.max_head_bytes)? else {
            if input.is_eof() {
                return Err(H1Error::UnexpectedEof);
            }
            return Ok(None);
        };
        input.consume(consumed);
        let (head, framing, keep_alive) = parse_response_head(
            block,
            self.limits.max_headers,
            self.response_to_head,
            self.response_to_connect,
        )?;
        self.state = match framing {
            BodyFraming::ContentLength(remaining) => State::Fixed {
                remaining,
                keep_alive,
            },
            BodyFraming::Chunked => State::ChunkSize { keep_alive },
            BodyFraming::UntilEof => State::UntilEof,
            BodyFraming::None => State::Fixed {
                remaining: 0,
                keep_alive,
            },
        };
        Ok(Some(H1Event::Head(head)))
    }
}

fn take_head_block(input: &RecvBuf, limit: usize) -> Result<Option<(IoBuf, usize)>, H1Error> {
    let bytes = input.copy_prefix(limit.saturating_add(1));
    if let Some(end) = find_bytes(&bytes, b"\r\n\r\n") {
        let consumed = end + 4;
        return Ok(Some((IoBuf::copy_from_slice(&bytes[..consumed]), consumed)));
    }
    if bytes.len() > limit {
        Err(H1Error::HeadTooLarge)
    } else {
        Ok(None)
    }
}

fn take_line(input: &mut RecvBuf, limit: usize) -> Result<Option<Vec<u8>>, H1Error> {
    let bytes = input.copy_prefix(limit.saturating_add(2));
    if let Some(end) = find_bytes(&bytes, b"\r\n") {
        input.consume(end + 2);
        return Ok(Some(bytes[..end].to_vec()));
    }
    if bytes.len() > limit {
        Err(H1Error::InvalidChunk)
    } else if input.is_eof() {
        Err(H1Error::UnexpectedEof)
    } else {
        Ok(None)
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let mut offset = 0;
    while offset + needle.len() <= haystack.len() {
        let relative = find_byte(&haystack[offset..], needle[0])?;
        offset += relative;
        if haystack[offset..].starts_with(needle) {
            return Some(offset);
        }
        offset += 1;
    }
    None
}

struct HeaderRange {
    name: Range<usize>,
    value: Range<usize>,
}

fn parse_response_head(
    block: IoBuf,
    max_headers: usize,
    response_to_head: bool,
    response_to_connect: bool,
) -> Result<(ResponseHead, BodyFraming, bool), H1Error> {
    validate_lines(block.as_ref())?;
    let status_end = find_bytes(block.as_ref(), b"\r\n").ok_or(H1Error::InvalidStatusLine)?;
    let status_line = &block.as_ref()[..status_end];
    let (http_11, status, reason) = parse_status_line(status_line)?;
    let ranges = parse_header_ranges(block.as_ref(), status_end + 2, max_headers)?;
    let framing = body_framing(
        block.as_ref(),
        &ranges,
        status,
        response_to_head,
        response_to_connect,
    )?;
    let keep_alive =
        framing != BodyFraming::UntilEof && connection_keep_alive(block.as_ref(), &ranges, http_11);
    let headers = ranges
        .iter()
        .map(|range| {
            (
                block.slice(range.name.clone()),
                block.slice(range.value.clone()),
            )
        })
        .collect();
    Ok((
        ResponseHead {
            status,
            reason: block.slice(reason),
            headers,
        },
        framing,
        keep_alive,
    ))
}

fn parse_trailers(block: IoBuf, max_headers: usize) -> Result<Vec<(IoBuf, IoBuf)>, H1Error> {
    validate_lines(block.as_ref())?;
    let ranges = parse_header_ranges(block.as_ref(), 0, max_headers)?;
    Ok(ranges
        .iter()
        .map(|range| {
            (
                block.slice(range.name.clone()),
                block.slice(range.value.clone()),
            )
        })
        .collect())
}

fn validate_lines(bytes: &[u8]) -> Result<(), H1Error> {
    if bytes.contains(&0)
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r'))
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
    {
        return Err(H1Error::InvalidHeader);
    }
    Ok(())
}

fn parse_status_line(line: &[u8]) -> Result<(bool, u16, Range<usize>), H1Error> {
    let http_11 = if line.starts_with(b"HTTP/1.1 ") {
        true
    } else if line.starts_with(b"HTTP/1.0 ") {
        false
    } else {
        return Err(H1Error::InvalidStatusLine);
    };
    if line.len() < 12 || !line[9..12].iter().all(u8::is_ascii_digit) {
        return Err(H1Error::InvalidStatusLine);
    }
    let status = std::str::from_utf8(&line[9..12])
        .unwrap()
        .parse()
        .map_err(|_| H1Error::InvalidStatusLine)?;
    let reason = if line.len() == 12 {
        12..12
    } else if line[12] == b' ' {
        13..line.len()
    } else {
        return Err(H1Error::InvalidStatusLine);
    };
    Ok((http_11, status, reason))
}

fn parse_header_ranges(
    bytes: &[u8],
    mut offset: usize,
    max_headers: usize,
) -> Result<Vec<HeaderRange>, H1Error> {
    let mut headers = Vec::new();
    while bytes.get(offset..offset + 2) != Some(b"\r\n") {
        let relative_end = find_bytes(&bytes[offset..], b"\r\n").ok_or(H1Error::InvalidHeader)?;
        let end = offset + relative_end;
        let line = &bytes[offset..end];
        if line
            .first()
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            return Err(H1Error::InvalidHeader);
        }
        let colon = line
            .iter()
            .position(|byte| *byte == b':')
            .ok_or(H1Error::InvalidHeader)?;
        if colon == 0
            || line[..colon].last().is_some_and(u8::is_ascii_whitespace)
            || !line[..colon].iter().all(|byte| is_token(*byte))
        {
            return Err(H1Error::InvalidHeader);
        }
        let mut value_start = offset + colon + 1;
        let mut value_end = end;
        while bytes
            .get(value_start)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            value_start += 1;
        }
        while value_end > value_start && matches!(bytes[value_end - 1], b' ' | b'\t') {
            value_end -= 1;
        }
        headers.push(HeaderRange {
            name: offset..offset + colon,
            value: value_start..value_end,
        });
        if headers.len() > max_headers {
            return Err(H1Error::TooManyHeaders);
        }
        offset = end + 2;
    }
    Ok(headers)
}

fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn header_values<'a>(
    bytes: &'a [u8],
    ranges: &'a [HeaderRange],
    name: &[u8],
) -> impl Iterator<Item = &'a [u8]> {
    ranges.iter().filter_map(move |range| {
        bytes[range.name.clone()]
            .eq_ignore_ascii_case(name)
            .then_some(&bytes[range.value.clone()])
    })
}

fn body_framing(
    bytes: &[u8],
    headers: &[HeaderRange],
    status: u16,
    response_to_head: bool,
    response_to_connect: bool,
) -> Result<BodyFraming, H1Error> {
    if response_to_head
        || (100..200).contains(&status)
        || matches!(status, 204 | 304)
        || (response_to_connect && (200..300).contains(&status))
    {
        return Ok(BodyFraming::None);
    }

    let transfer_encoding: Vec<_> = header_values(bytes, headers, b"transfer-encoding")
        .flat_map(|value| value.split(|byte| *byte == b','))
        .map(trim_ows)
        .collect();
    if !transfer_encoding.is_empty() {
        if transfer_encoding
            .last()
            .is_some_and(|encoding| encoding.eq_ignore_ascii_case(b"chunked"))
        {
            return Ok(BodyFraming::Chunked);
        }
        return Ok(BodyFraming::UntilEof);
    }

    let mut lengths = header_values(bytes, headers, b"content-length")
        .flat_map(|value| value.split(|byte| *byte == b','))
        .map(trim_ows);
    let Some(first) = lengths.next() else {
        return Ok(BodyFraming::UntilEof);
    };
    let length = parse_content_length(first)?;
    for other in lengths {
        if parse_content_length(other)? != length {
            return Err(H1Error::InvalidContentLength);
        }
    }
    Ok(BodyFraming::ContentLength(length))
}

fn parse_content_length(value: &[u8]) -> Result<u64, H1Error> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return Err(H1Error::InvalidContentLength);
    }
    std::str::from_utf8(value)
        .unwrap()
        .parse()
        .map_err(|_| H1Error::InvalidContentLength)
}

fn trim_ows(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[1..];
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[..value.len() - 1];
    }
    value
}

fn connection_keep_alive(bytes: &[u8], headers: &[HeaderRange], http_11: bool) -> bool {
    let tokens: Vec<_> = header_values(bytes, headers, b"connection")
        .flat_map(|value| value.split(|byte| *byte == b','))
        .map(trim_ows)
        .collect();
    if tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case(b"close"))
    {
        return false;
    }
    http_11
        || tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case(b"keep-alive"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_bytewise(bytes: &[u8], eof: bool) -> Result<Vec<String>, H1Error> {
        let mut parser = H1Parser::new();
        let mut input = RecvBuf::default();
        let mut events = Vec::new();
        for byte in bytes {
            input.push(IoBuf::copy_from_slice(&[*byte]));
            while let Some(event) = parser.decode(&mut input)? {
                describe(event, &mut events);
            }
        }
        if eof {
            input.set_eof();
            while let Some(event) = parser.decode(&mut input)? {
                describe(event, &mut events);
            }
        }
        Ok(events)
    }

    fn describe(event: H1Event, output: &mut Vec<String>) {
        match event {
            H1Event::Head(head) => output.push(format!("head:{}", head.status)),
            H1Event::Body(body) => {
                output.push(format!("body:{}", String::from_utf8_lossy(body.as_ref())))
            }
            H1Event::End {
                keep_alive,
                trailers,
            } => output.push(format!("end:{keep_alive}:{}", trailers.len())),
        }
    }

    #[test]
    fn content_length_streams_across_every_boundary() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let events = decode_bytewise(raw, false).unwrap();
        assert_eq!(events.first().unwrap(), "head:200");
        assert_eq!(
            events[1..6],
            ["body:h", "body:e", "body:l", "body:l", "body:o"]
        );
        assert_eq!(events.last().unwrap(), "end:true:0");
    }

    #[test]
    fn chunked_body_and_trailers() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;foo=bar\r\nhello\r\n0\r\nX-End: yes\r\n\r\n";
        let events = decode_bytewise(raw, false).unwrap();
        assert_eq!(events.first().unwrap(), "head:200");
        assert!(events.iter().any(|event| event == "body:h"));
        assert_eq!(events.last().unwrap(), "end:true:1");
    }

    #[test]
    fn eof_terminated_body_forces_close() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\nhello";
        let events = decode_bytewise(raw, true).unwrap();
        assert_eq!(events.last().unwrap(), "end:false:0");
    }

    #[test]
    fn framing_rejects_smuggling_inputs() {
        for raw in [
            &b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabc"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length : 3\r\n\r\nabc"[..],
            &b"HTTP/1.1 200 OK\r\nX: ok\r\n folded\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\nContent-Length: 0\n\n"[..],
        ] {
            assert!(decode_bytewise(raw, true).is_err());
        }
    }

    #[test]
    fn transfer_encoding_overrides_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 999\r\n\r\n1\r\nx\r\n0\r\n\r\n";
        assert!(
            decode_bytewise(raw, false)
                .unwrap()
                .iter()
                .any(|event| event == "body:x")
        );
    }
}
