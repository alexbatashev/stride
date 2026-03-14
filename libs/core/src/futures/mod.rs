use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};

pub type BoxFuture<'a, T> = ::futures::future::BoxFuture<'a, T>;

pub trait Stream {
    type Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}

impl<T> Stream for T
where
    T: ::futures::Stream + ?Sized,
{
    type Item = <T as ::futures::Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        <T as ::futures::Stream>::poll_next(self, cx)
    }
}

pub trait StreamExt: Stream {
    fn next<'a>(&'a mut self) -> BoxFuture<'a, Option<Self::Item>>
    where
        Self: Unpin + Send + 'a,
    {
        Box::pin(poll_fn(move |cx| Pin::new(&mut *self).poll_next(cx)))
    }
}

impl<T> StreamExt for T where T: Stream + ?Sized {}
