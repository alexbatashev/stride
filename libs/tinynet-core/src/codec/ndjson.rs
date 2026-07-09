use crate::utils::find_byte;

#[derive(Default)]
pub struct NdjsonDecoder {
    buffer: Vec<u8>,
    start: usize,
}

impl NdjsonDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.compact();
        self.buffer.extend_from_slice(chunk);
    }

    fn decode_next(&mut self) -> Option<Vec<u8>> {
        loop {
            let end = self.start + find_byte(&self.buffer[self.start..], b'\n')?;
            let mut line_end = end;
            if line_end > self.start && self.buffer[line_end - 1] == b'\r' {
                line_end -= 1;
            }
            let line = self.buffer[self.start..line_end].to_vec();
            self.start = end + 1;
            if !line.is_empty() {
                return Some(line);
            }
        }
    }

    fn compact(&mut self) {
        if self.start > 0 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
    }
}

impl Iterator for NdjsonDecoder {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.decode_next()
    }
}

#[cfg(test)]
mod tests {
    use super::NdjsonDecoder;

    fn decode(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut decoder = NdjsonDecoder::new();
        let mut lines = Vec::new();
        for chunk in chunks {
            decoder.push(chunk);
            lines.extend(decoder.by_ref());
        }
        lines
    }

    #[test]
    fn splits_lines_and_skips_empty_lines() {
        assert_eq!(
            decode(&[b"{\"a\":1}\r\n\n{\"b\":2}\n"]),
            [b"{\"a\":1}".to_vec(), b"{\"b\":2}".to_vec()]
        );
    }

    #[test]
    fn every_split_matches_one_shot() {
        let input = b"one\r\n\ntwo\nthree\n";
        let expected = decode(&[input]);
        for split in 0..=input.len() {
            assert_eq!(decode(&[&input[..split], &input[split..]]), expected);
        }
    }
}
