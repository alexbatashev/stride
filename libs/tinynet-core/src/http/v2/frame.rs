pub(crate) const DEFAULT_MAX_FRAME_SIZE: usize = 16_384;
pub(crate) const HEADER_LEN: usize = 9;

pub(crate) const DATA: u8 = 0x0;
pub(crate) const HEADERS: u8 = 0x1;
pub(crate) const SETTINGS: u8 = 0x4;
pub(crate) const CONTINUATION: u8 = 0x9;

pub(crate) const FLAG_END_STREAM: u8 = 0x1;
pub(crate) const FLAG_END_HEADERS: u8 = 0x4;

pub(crate) fn write(buf: &mut Vec<u8>, frame_type: u8, flags: u8, stream_id: u32, payload: &[u8]) {
    let len = payload.len();
    debug_assert!(len <= 0x00FF_FFFF);

    buf.push((len >> 16) as u8);
    buf.push((len >> 8) as u8);
    buf.push(len as u8);
    buf.push(frame_type);
    buf.push(flags);
    buf.extend_from_slice(&(stream_id & 0x7FFF_FFFF).to_be_bytes());
    buf.extend_from_slice(payload);
}

pub(crate) fn write_chunked(
    buf: &mut Vec<u8>,
    frame_type: u8,
    final_flags: u8,
    stream_id: u32,
    payload: &[u8],
) {
    if payload.is_empty() {
        write(buf, frame_type, final_flags, stream_id, payload);
        return;
    }

    let mut remaining = payload;
    while remaining.len() > DEFAULT_MAX_FRAME_SIZE {
        let (chunk, rest) = remaining.split_at(DEFAULT_MAX_FRAME_SIZE);
        write(buf, frame_type, 0, stream_id, chunk);
        remaining = rest;
    }
    write(buf, frame_type, final_flags, stream_id, remaining);
}

pub(crate) fn parse_header(buf: &[u8]) -> (usize, u8, u8, u32) {
    let len = (buf[0] as usize) << 16 | (buf[1] as usize) << 8 | buf[2] as usize;
    let frame_type = buf[3];
    let flags = buf[4];
    let stream_id = u32::from_be_bytes([buf[5] & 0x7F, buf[6], buf[7], buf[8]]);
    (len, frame_type, flags, stream_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_frames_respect_default_size() {
        let payload = vec![b'x'; DEFAULT_MAX_FRAME_SIZE + 1];
        let mut out = Vec::new();

        write_chunked(&mut out, DATA, FLAG_END_STREAM, 1, &payload);

        let (first_len, first_type, first_flags, first_sid) = parse_header(&out);
        assert_eq!(first_len, DEFAULT_MAX_FRAME_SIZE);
        assert_eq!(first_type, DATA);
        assert_eq!(first_flags, 0);
        assert_eq!(first_sid, 1);

        let second = HEADER_LEN + DEFAULT_MAX_FRAME_SIZE;
        let (second_len, second_type, second_flags, second_sid) = parse_header(&out[second..]);
        assert_eq!(second_len, 1);
        assert_eq!(second_type, DATA);
        assert_eq!(second_flags, FLAG_END_STREAM);
        assert_eq!(second_sid, 1);
    }
}
