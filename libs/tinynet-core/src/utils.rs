/// A wrapper type that can hold static, borrowed, or owned string data.
#[derive(Clone)]
pub enum StrLike<'a> {
    Str(&'static str),
    Borrowed(&'a str),
    Owned(String),
}

/// Finds n-th occurence of pattern character inside source string
pub const fn find_nth(src: &str, pattern: char, n: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut x = 1;

    while i < bytes.len() {
        if bytes[i] as char == pattern {
            if x == n {
                return Some(i);
            } else {
                x += 1;
            }
        }
        i += 1;
    }

    None
}

pub(crate) fn find_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    let mut offset = 0;
    while offset + 32 <= bytes.len() {
        let block = &bytes[offset..offset + 32];
        if let Some(index) = block.iter().position(|byte| *byte == needle) {
            return Some(offset + index);
        }
        offset += 32;
    }
    bytes[offset..]
        .iter()
        .position(|byte| *byte == needle)
        .map(|index| offset + index)
}

pub(crate) fn find_line_end(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while offset + 32 <= bytes.len() {
        let block = &bytes[offset..offset + 32];
        if let Some(index) = block.iter().position(|byte| matches!(*byte, b'\r' | b'\n')) {
            return Some(offset + index);
        }
        offset += 32;
    }
    bytes[offset..]
        .iter()
        .position(|byte| matches!(*byte, b'\r' | b'\n'))
        .map(|index| offset + index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedOut;

impl std::fmt::Display for TimedOut {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("operation timed out")
    }
}

impl std::error::Error for TimedOut {}

pub async fn timeout<F: std::future::Future>(
    duration: std::time::Duration,
    future: F,
) -> Result<F::Output, TimedOut> {
    let mut future = std::pin::pin!(future);
    let mut timer = std::pin::pin!(crate::sleep(duration));
    std::future::poll_fn(|context| {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(context) {
            return std::task::Poll::Ready(Ok(output));
        }
        if timer.as_mut().poll(context).is_ready() {
            return std::task::Poll::Ready(Err(TimedOut));
        }
        std::task::Poll::Pending
    })
    .await
}

impl<'a> StrLike<'a> {
    pub fn into_owned(self) -> StrLike<'static> {
        match self {
            StrLike::Str(s) => StrLike::Str(s),
            StrLike::Borrowed(s) => StrLike::Owned(s.to_owned()),
            StrLike::Owned(s) => StrLike::Owned(s),
        }
    }
}

impl<'a> From<&'a str> for StrLike<'a> {
    fn from(value: &'a str) -> Self {
        StrLike::Borrowed(value)
    }
}

impl<'a> From<String> for StrLike<'a> {
    fn from(value: String) -> Self {
        StrLike::Owned(value)
    }
}

impl AsRef<str> for StrLike<'_> {
    fn as_ref(&self) -> &str {
        match self {
            StrLike::Str(s) => s,
            StrLike::Borrowed(s) => s,
            StrLike::Owned(s) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::utils::{find_byte, find_line_end, find_nth, timeout};

    #[test]
    fn nth_is_found() {
        let x = find_nth("https://example.com/", '/', 3);
        assert_eq!(x, Some(19));
    }

    #[test]
    fn block_scans_match_naive_search() {
        let mut bytes = vec![b'x'; 97];
        for index in 0..bytes.len() {
            bytes[index] = b'\n';
            assert_eq!(
                find_byte(&bytes, b'\n'),
                bytes.iter().position(|b| *b == b'\n')
            );
            assert_eq!(
                find_line_end(&bytes),
                bytes.iter().position(|b| matches!(*b, b'\r' | b'\n'))
            );
            bytes[index] = b'x';
        }
    }

    #[tokio::test]
    async fn timeout_completes_or_expires() {
        assert_eq!(
            timeout(Duration::from_millis(20), async { 7 })
                .await
                .unwrap(),
            7
        );
        assert!(
            timeout(
                Duration::from_millis(10),
                crate::sleep(Duration::from_secs(1))
            )
            .await
            .is_err()
        );
    }
}
