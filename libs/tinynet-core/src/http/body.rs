use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use crate::{
    buf::{IoBuf, IoBufMut},
    client::ClientIo,
    http::{
        Bytes,
        v1_conn::{H1Event, H1Parser, RecvBuf, ResponseHead},
    },
};

pub struct StreamingResponse<'a> {
    pub head: ResponseHead,
    pub body: Body<'a>,
}

pub struct Body<'a> {
    io: &'a mut ClientIo,
    parser: H1Parser,
    input: RecvBuf,
    read_buffer: IoBufMut,
    timeout: Option<Pin<Box<crate::Sleep>>>,
    reusable: &'a mut bool,
    done: bool,
}

impl<'a> Body<'a> {
    pub(crate) fn new(
        io: &'a mut ClientIo,
        timeout: Option<Duration>,
        reusable: &'a mut bool,
    ) -> Self {
        Self {
            io,
            parser: H1Parser::new(),
            input: RecvBuf::default(),
            read_buffer: IoBufMut::with_capacity(16 * 1024),
            timeout: timeout.map(|duration| Box::pin(crate::sleep(duration))),
            reusable,
            done: false,
        }
    }

    pub(crate) async fn head(&mut self) -> io::Result<ResponseHead> {
        match std::future::poll_fn(|context| self.poll_event(context)).await? {
            H1Event::Head(head) => Ok(head),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "body before head",
            )),
        }
    }

    pub async fn chunk(&mut self) -> io::Result<Option<IoBuf>> {
        std::future::poll_fn(|context| self.poll_chunk(context)).await
    }

    pub fn poll_chunk(&mut self, context: &mut Context<'_>) -> Poll<io::Result<Option<IoBuf>>> {
        if self.done {
            return Poll::Ready(Ok(None));
        }
        loop {
            match self.poll_event(context) {
                Poll::Ready(Ok(H1Event::Body(chunk))) => return Poll::Ready(Ok(Some(chunk))),
                Poll::Ready(Ok(H1Event::End { keep_alive, .. })) => {
                    self.done = true;
                    *self.reusable = keep_alive;
                    return Poll::Ready(Ok(None));
                }
                Poll::Ready(Ok(H1Event::Head(_))) => continue,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    pub async fn collect(mut self) -> io::Result<Bytes<'static>> {
        let mut collected = Vec::new();
        while let Some(chunk) = self.chunk().await? {
            collected.extend_from_slice(chunk.as_ref());
        }
        Ok(Bytes::Owned(collected))
    }

    fn poll_event(&mut self, context: &mut Context<'_>) -> Poll<io::Result<H1Event>> {
        loop {
            match self.parser.decode(&mut self.input) {
                Ok(Some(event)) => return Poll::Ready(Ok(event)),
                Err(error) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("HTTP/1 parse error: {error:?}"),
                    )));
                }
                Ok(None) => {}
            }

            if self
                .timeout
                .as_mut()
                .is_some_and(|timeout| timeout.as_mut().poll(context).is_ready())
            {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "request timed out",
                )));
            }

            let (io, read_buffer) = (&mut self.io, &mut self.read_buffer);
            match io.poll_read(context, read_buffer.as_mut_slice()) {
                Poll::Ready(Ok(0)) => self.input.set_eof(),
                Poll::Ready(Ok(read)) => {
                    read_buffer.set_len(read);
                    let replacement = IoBufMut::with_capacity(16 * 1024);
                    let filled = std::mem::replace(read_buffer, replacement).freeze();
                    self.input.push(filled);
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl Drop for Body<'_> {
    fn drop(&mut self) {
        if !self.done {
            *self.reusable = false;
        }
    }
}

impl std::future::Future for &mut Body<'_> {
    type Output = io::Result<Option<IoBuf>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().poll_chunk(context)
    }
}
