use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::task::Poll;
use uniffi::{FfiConverter, Lower, RustBuffer, TypeId};

#[must_use]
pub struct BoxStream<T, E>
where
    E: std::error::Error,
{
    inner: Pin<Box<dyn Stream<Item = Result<T, E>> + Send + 'static>>,
}

impl<T, E> BoxStream<T, E>
where
    E: std::error::Error,
{
    pub fn new(inner: Pin<Box<dyn Stream<Item = Result<T, E>> + Send + 'static>>) -> Self {
        Self { inner }
    }
}

impl<T, E> Stream for BoxStream<T, E>
where
    E: std::error::Error,
{
    type Item = Result<T, E>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

unsafe impl<T, E, UT> FfiConverter<UT> for BoxStream<T, E>
where
    T: FfiConverter<UT> + Send + 'static,
    E: FfiConverter<UT> + std::error::Error + Send + 'static,
{
    type FfiType = RustBuffer;

    fn lower(obj: Self) -> Self::FfiType {
        let mut buf = Vec::new();
        Self::write(obj, &mut buf);
        RustBuffer::from_vec(buf)
    }

    fn try_lift(v: Self::FfiType) -> uniffi::Result<Self> {
        let bytes = v.destroy_into_vec();
        let mut slice = bytes.as_slice();
        let stream = Self::try_read(&mut slice)?;
        if !slice.is_empty() {
            return Err(uniffi::deps::anyhow::anyhow!(
                "junk data left in buffer after lifting BoxStream"
            ));
        }
        Ok(stream)
    }

    fn write(obj: Self, buf: &mut Vec<u8>) {
        let items = futures::executor::block_on(async move { obj.inner.collect::<Vec<T>>().await });
        let len = i32::try_from(items.len()).expect("BoxStream item count exceeds i32::MAX");
        buf.extend_from_slice(&len.to_be_bytes());
        for item in items {
            match item {
                Ok(value) => {
                    buf.push(1);
                    <T as FfiConverter<UT>>::write(value, buf);
                }
                Err(error) => {
                    buf.push(0);
                    <E as FfiConverter<UT>>::write(error, buf);
                }
            }
        }
    }

    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Self> {
        if buf.len() < 4 {
            return Err(uniffi::deps::anyhow::anyhow!(
                "buffer is too small to read BoxStream length"
            ));
        }

        let (head, tail) = buf.split_at(4);
        let len = i32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        if len < 0 {
            return Err(uniffi::deps::anyhow::anyhow!(
                "negative BoxStream length is invalid"
            ));
        }
        *buf = tail;

        let mut items = Vec::with_capacity(len as usize);
        for _ in 0..len {
            if buf.is_empty() {
                return Err(uniffi::deps::anyhow::anyhow!(
                    "buffer is too small to read BoxStream item tag"
                ));
            }
            let (tag, rest) = buf.split_at(1);
            *buf = rest;
            if tag[0] == 1 {
                items.push(Ok(<T as FfiConverter<UT>>::try_read(buf)?));
            } else {
                items.push(Err(<E as FfiConverter<UT>>::try_read(buf)?));
            }
        }

        Ok(Self::new(Box::pin(futures::stream::iter(items))))
    }

    const TYPE_ID_META: uniffi::MetadataBuffer = <Vec<u8> as TypeId<UT>>::TYPE_ID_META;
}

uniffi::derive_ffi_traits!(
    impl<T, E, UT> Lower<UT> for BoxStream<T, E>
    where
        BoxStream<T, E>: FfiConverter<UT>,
        T: FfiConverter<UT> + Send + 'static,
        E: FfiConverter<UT> + std::error::Error + Send + 'static
);
uniffi::derive_ffi_traits!(
    impl<T, E, UT> LowerReturn<UT> for BoxStream<T, E>
    where
        BoxStream<T, E>: Lower<UT>,
        T: FfiConverter<UT> + Send + 'static,
        E: FfiConverter<UT> + std::error::Error + Send + 'static
);
uniffi::derive_ffi_traits!(
    impl<T, E, UT> LowerError<UT> for BoxStream<T, E>
    where
        BoxStream<T, E>: Lower<UT>,
        T: FfiConverter<UT> + Send + 'static,
        E: FfiConverter<UT> + std::error::Error + Send + 'static
);
uniffi::derive_ffi_traits!(
    impl<T, E, UT> TypeId<UT> for BoxStream<T, E>
    where
        BoxStream<T, E>: FfiConverter<UT>,
        T: FfiConverter<UT> + Send + 'static,
        E: FfiConverter<UT> + std::error::Error + Send + 'static
);
