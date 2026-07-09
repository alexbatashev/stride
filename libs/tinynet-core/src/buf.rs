use std::{
    io::IoSlice,
    ops::Range,
    sync::{Arc, LazyLock, Mutex, Weak},
};

const SIZE_CLASSES: [usize; 3] = [4 * 1024, 16 * 1024, 64 * 1024];
const MAX_POOLED_PER_CLASS: usize = 64;

struct PoolShared {
    buffers: [Mutex<Vec<Box<[u8]>>>; SIZE_CLASSES.len()],
}

impl PoolShared {
    fn new() -> Self {
        Self {
            buffers: std::array::from_fn(|_| Mutex::new(Vec::new())),
        }
    }

    fn acquire(self: &Arc<Self>, requested: usize) -> IoBufMut {
        let class = SIZE_CLASSES.iter().position(|size| *size >= requested);
        let data = class
            .and_then(|index| self.buffers[index].lock().unwrap().pop())
            .unwrap_or_else(|| {
                vec![0; class.map_or(requested, |index| SIZE_CLASSES[index])].into_boxed_slice()
            });
        IoBufMut {
            data,
            filled: 0,
            pool: class.map(|_| Arc::downgrade(self)),
            class,
        }
    }
}

static POOL: LazyLock<Arc<PoolShared>> = LazyLock::new(|| Arc::new(PoolShared::new()));

struct IoBufInner {
    data: Option<Box<[u8]>>,
    pool: Option<Weak<PoolShared>>,
    class: Option<usize>,
}

impl Drop for IoBufInner {
    fn drop(&mut self) {
        let (Some(pool), Some(class), Some(data)) = (
            self.pool.as_ref().and_then(Weak::upgrade),
            self.class,
            self.data.take(),
        ) else {
            return;
        };
        let mut buffers = pool.buffers[class].lock().unwrap();
        if buffers.len() < MAX_POOLED_PER_CLASS {
            buffers.push(data);
        }
    }
}

#[derive(Clone)]
pub struct IoBuf {
    inner: Arc<IoBufInner>,
    start: usize,
    len: usize,
}

impl IoBuf {
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        let mut buffer = IoBufMut::with_capacity(bytes.len());
        buffer.as_mut_slice()[..bytes.len()].copy_from_slice(bytes);
        buffer.set_len(bytes.len());
        buffer.freeze()
    }

    pub fn slice(&self, range: Range<usize>) -> Self {
        assert!(range.start <= range.end && range.end <= self.len);
        Self {
            inner: Arc::clone(&self.inner),
            start: self.start + range.start,
            len: range.end - range.start,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl AsRef<[u8]> for IoBuf {
    fn as_ref(&self) -> &[u8] {
        &self.inner.data.as_ref().unwrap()[self.start..self.start + self.len]
    }
}

pub struct IoBufMut {
    data: Box<[u8]>,
    filled: usize,
    pool: Option<Weak<PoolShared>>,
    class: Option<usize>,
}

impl IoBufMut {
    pub fn with_capacity(capacity: usize) -> Self {
        POOL.acquire(capacity)
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn set_len(&mut self, filled: usize) {
        assert!(filled <= self.data.len());
        self.filled = filled;
    }

    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    pub fn freeze(self) -> IoBuf {
        let len = self.filled;
        IoBuf {
            inner: Arc::new(IoBufInner {
                data: Some(self.data),
                pool: self.pool,
                class: self.class,
            }),
            start: 0,
            len,
        }
    }
}

enum Segment {
    Static(&'static [u8]),
    Owned(Vec<u8>),
    Shared(IoBuf),
}

#[derive(Default)]
pub struct WriteBuf {
    segments: Vec<Segment>,
    head: usize,
    offset: usize,
}

impl WriteBuf {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_static(&mut self, bytes: &'static [u8]) {
        if !bytes.is_empty() {
            self.segments.push(Segment::Static(bytes));
        }
    }

    pub fn put(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        match self.segments.last_mut() {
            Some(Segment::Owned(tail)) => tail.extend_from_slice(bytes),
            _ => self.segments.push(Segment::Owned(bytes.to_vec())),
        }
    }

    pub fn put_shared(&mut self, bytes: IoBuf) {
        if !bytes.is_empty() {
            self.segments.push(Segment::Shared(bytes));
        }
    }

    pub fn as_ioslices(&self) -> Vec<IoSlice<'_>> {
        self.segments[self.head..]
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let bytes = match segment {
                    Segment::Static(bytes) => *bytes,
                    Segment::Owned(bytes) => bytes.as_slice(),
                    Segment::Shared(bytes) => bytes.as_ref(),
                };
                IoSlice::new(if index == 0 {
                    &bytes[self.offset..]
                } else {
                    bytes
                })
            })
            .collect()
    }

    pub fn advance(&mut self, mut count: usize) {
        while count > 0 && self.head < self.segments.len() {
            let remaining = self.segment(self.head).len() - self.offset;
            if count < remaining {
                self.offset += count;
                return;
            }
            count -= remaining;
            self.head += 1;
            self.offset = 0;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.segments.len()
    }

    pub fn concat(&self) -> Vec<u8> {
        self.as_ioslices()
            .iter()
            .flat_map(|slice| slice.iter().copied())
            .collect()
    }

    fn segment(&self, index: usize) -> &[u8] {
        match &self.segments[index] {
            Segment::Static(bytes) => bytes,
            Segment::Owned(bytes) => bytes,
            Segment::Shared(bytes) => bytes.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_handles_partial_segments() {
        let mut buffer = WriteBuf::new();
        buffer.put_static(b"head");
        buffer.put(b"body");
        buffer.advance(2);
        assert_eq!(buffer.concat(), b"adbody");
        buffer.advance(3);
        assert_eq!(buffer.concat(), b"ody");
        buffer.advance(3);
        assert!(buffer.is_empty());
    }

    #[test]
    fn shared_slices_alias_frozen_storage() {
        let buffer = IoBuf::copy_from_slice(b"abcdef");
        let slice = buffer.slice(1..4);
        assert_eq!(slice.as_ref(), b"bcd");
        assert_eq!(buffer.as_ref(), b"abcdef");
        assert_eq!(Arc::strong_count(&buffer.inner), 2);
    }

    #[test]
    fn pooled_storage_is_reused() {
        let pool = Arc::new(PoolShared::new());
        let pointer = {
            let mut buffer = pool.acquire(100);
            let pointer = buffer.as_mut_slice().as_ptr();
            buffer.set_len(1);
            drop(buffer.freeze());
            pointer
        };
        let mut reused = pool.acquire(100);
        assert_eq!(reused.capacity(), SIZE_CLASSES[0]);
        assert_eq!(reused.as_mut_slice().as_ptr(), pointer);
    }
}
