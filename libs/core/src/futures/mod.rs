use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll};
use uniffi::{FfiConverter, TypeId};

mod stream;
pub use stream::BoxStream;

pub struct BoxFuture<'a, T>(Pin<Box<dyn Future<Output = T> + Send + 'a>>);

impl<'a, T> BoxFuture<'a, T> {
    pub fn new(inner: Pin<Box<dyn Future<Output = T> + Send + 'a>>) -> Self {
        Self(inner)
    }

    pub fn from_future<F>(value: F) -> Self
    where
        F: Future<Output = T> + Send + 'a,
    {
        Self(Box::pin(value))
    }
}

impl<'a, T> Future for BoxFuture<'a, T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

unsafe impl<'a, T, UT> FfiConverter<UT> for BoxFuture<'a, T>
where
    T: FfiConverter<UT> + Send + 'a,
{
    type FfiType = <T as FfiConverter<UT>>::FfiType;

    fn lower(obj: Self) -> Self::FfiType {
        let value = futures::executor::block_on(obj);
        <T as FfiConverter<UT>>::lower(value)
    }

    fn try_lift(v: Self::FfiType) -> uniffi::Result<Self> {
        let value = <T as FfiConverter<UT>>::try_lift(v)?;
        Ok(BoxFuture::from_future(async move { value }))
    }

    fn write(obj: Self, buf: &mut Vec<u8>) {
        let value = futures::executor::block_on(obj);
        <T as FfiConverter<UT>>::write(value, buf);
    }

    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Self> {
        let value = <T as FfiConverter<UT>>::try_read(buf)?;
        Ok(BoxFuture::from_future(async move { value }))
    }

    const TYPE_ID_META: uniffi::MetadataBuffer = <T as FfiConverter<UT>>::TYPE_ID_META;
}

pub trait StreamExt: ::futures::Stream {
    fn next_boxed<'a>(&'a mut self) -> BoxFuture<'a, Option<Self::Item>>
    where
        Self: Unpin + Send + 'a,
    {
        BoxFuture::from_future(poll_fn(move |cx| Pin::new(&mut *self).poll_next(cx)))
    }
}

impl<T> StreamExt for T where T: ::futures::Stream + ?Sized {}
