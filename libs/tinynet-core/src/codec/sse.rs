use crate::utils::find_line_end;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

#[derive(Default)]
struct PartialEvent {
    event: Option<String>,
    data: String,
    retry: Option<u64>,
}

pub struct SseDecoder {
    buffer: Vec<u8>,
    start: usize,
    at_start: bool,
    skip_lf: bool,
    last_event_id: Option<String>,
    event: PartialEvent,
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            start: 0,
            at_start: true,
            skip_lf: false,
            last_event_id: None,
            event: PartialEvent::default(),
        }
    }
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.compact();
        self.buffer.extend_from_slice(chunk);
    }

    fn decode_next(&mut self) -> Option<SseEvent> {
        loop {
            if !self.prepare_start() {
                return None;
            }
            if self.skip_lf {
                match self.buffer.get(self.start) {
                    Some(b'\n') => self.start += 1,
                    Some(_) => {}
                    None => return None,
                }
                self.skip_lf = false;
            }

            let relative_end = find_line_end(&self.buffer[self.start..])?;
            let end = self.start + relative_end;
            let line = self.buffer[self.start..end].to_vec();
            let ending = self.buffer[end];
            self.start = end + 1;
            if ending == b'\r' {
                if self.buffer.get(self.start) == Some(&b'\n') {
                    self.start += 1;
                } else if self.start == self.buffer.len() {
                    self.skip_lf = true;
                }
            }

            if line.is_empty() {
                if let Some(event) = self.dispatch() {
                    return Some(event);
                }
            } else {
                self.process_line(&line);
            }
        }
    }

    fn prepare_start(&mut self) -> bool {
        if !self.at_start {
            return true;
        }
        let remaining = &self.buffer[self.start..];
        let bom = b"\xef\xbb\xbf";
        if remaining.len() < bom.len() && bom.starts_with(remaining) {
            return false;
        }
        if remaining.starts_with(bom) {
            self.start += bom.len();
        }
        self.at_start = false;
        true
    }

    fn process_line(&mut self, line: &[u8]) {
        if line.first() == Some(&b':') {
            return;
        }
        let colon = line.iter().position(|byte| *byte == b':');
        let (field, mut value) = match colon {
            Some(index) => (&line[..index], &line[index + 1..]),
            None => (line, &[][..]),
        };
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let value = String::from_utf8_lossy(value);
        match field {
            b"event" => self.event.event = Some(value.into_owned()),
            b"data" => {
                self.event.data.push_str(&value);
                self.event.data.push('\n');
            }
            b"id" if !value.as_bytes().contains(&0) => {
                self.last_event_id = Some(value.into_owned());
            }
            b"retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                if let Ok(retry) = value.parse() {
                    self.event.retry = Some(retry);
                }
            }
            _ => {}
        }
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.event.data.is_empty() {
            self.event.event = None;
            return None;
        }
        self.event.data.pop();
        let event = SseEvent {
            event: self.event.event.take(),
            data: std::mem::take(&mut self.event.data),
            id: self.last_event_id.clone(),
            retry: self.event.retry.take(),
        };
        Some(event)
    }

    fn compact(&mut self) {
        if self.start > 0 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
    }
}

impl Iterator for SseDecoder {
    type Item = SseEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.decode_next()
    }
}

#[cfg(test)]
mod tests {
    use super::{SseDecoder, SseEvent};

    fn decode(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        for chunk in chunks {
            decoder.push(chunk);
            events.extend(decoder.by_ref());
        }
        events
    }

    #[test]
    fn decodes_eventsource_fields() {
        let events = decode(&[
            b"\xef\xbb\xbf: comment\r\nevent: add\rdata: one\ndata:two\nid: 7\nretry: 1000\n\n",
        ]);
        assert_eq!(
            events,
            [SseEvent {
                event: Some("add".into()),
                data: "one\ntwo".into(),
                id: Some("7".into()),
                retry: Some(1000),
            }]
        );
    }

    #[test]
    fn handles_fields_without_colons_and_persistent_ids() {
        let events = decode(&[b"id: first\ndata\n\ndata: second\n\n"]);
        assert_eq!(events[0].data, "");
        assert_eq!(events[0].id.as_deref(), Some("first"));
        assert_eq!(events[1].data, "second");
        assert_eq!(events[1].id.as_deref(), Some("first"));
    }

    #[test]
    fn every_split_matches_one_shot() {
        let input = b"\xef\xbb\xbfdata: one\r\ndata: two\r\rdata: three\n\n";
        let expected = decode(&[input]);
        for split in 0..=input.len() {
            assert_eq!(decode(&[&input[..split], &input[split..]]), expected);
        }
    }
}
