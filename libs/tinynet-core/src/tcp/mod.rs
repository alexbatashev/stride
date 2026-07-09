use std::{
    future::Future,
    io::{self, IoSlice},
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use crate::reactor::REACTOR;
use crate::sys::Socket;

pub struct TcpStream {
    socket: Socket,
}

impl TcpStream {
    pub fn connect(addr: SocketAddr) -> ConnectFuture {
        ConnectFuture { addr, socket: None }
    }

    pub fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadFuture<'a> {
        ReadFuture {
            socket: &self.socket,
            buf,
        }
    }

    pub(crate) fn poll_read(
        &mut self,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.socket.recv(buffer) {
            Ok(read) => Poll::Ready(Ok(read)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_read(self.socket.handle(), context.waker().clone());
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    #[cfg(feature = "rustls")]
    pub(crate) fn poll_write(
        &mut self,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.socket.send(buffer) {
            Ok(written) => Poll::Ready(Ok(written)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_write(self.socket.handle(), context.waker().clone());
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    pub fn write<'a>(&'a mut self, buf: &'a [u8]) -> WriteFuture<'a> {
        WriteFuture {
            socket: &self.socket,
            buf,
        }
    }

    pub fn write_vectored<'a>(&'a mut self, buffers: &'a [IoSlice<'a>]) -> WriteVectoredFuture<'a> {
        WriteVectoredFuture {
            socket: &self.socket,
            buffers,
        }
    }
}

// ---------------------------------------------------------------------------
// TcpListener
// ---------------------------------------------------------------------------

pub struct TcpListener {
    socket: Socket,
}

impl TcpListener {
    /// Bind and start listening. This is synchronous — bind/listen don't wait
    /// for external I/O. Use `accept()` to asynchronously receive connections.
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        let socket = Socket::new_tcp(addr)?;
        socket.bind(addr)?;
        socket.listen(128)?;
        Ok(TcpListener { socket })
    }

    pub fn accept(&mut self) -> AcceptFuture<'_> {
        AcceptFuture {
            socket: &self.socket,
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

pub struct AcceptFuture<'a> {
    socket: &'a Socket,
}

impl Future for AcceptFuture<'_> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.socket.accept() {
            Ok((socket, addr)) => Poll::Ready(Ok((TcpStream { socket }, addr))),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_read(self.socket.handle(), cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// ConnectFuture
// ---------------------------------------------------------------------------

pub struct ConnectFuture {
    addr: SocketAddr,
    socket: Option<Socket>,
}

impl Future for ConnectFuture {
    type Output = io::Result<TcpStream>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.socket.is_none() {
            let socket = match Socket::new_tcp(self.addr) {
                Ok(s) => s,
                Err(e) => return Poll::Ready(Err(e)),
            };
            match socket.connect(self.addr) {
                Ok(()) => return Poll::Ready(Ok(TcpStream { socket })),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    REACTOR.register_write(socket.handle(), cx.waker().clone());
                    self.socket = Some(socket);
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }

        // Write-ready: the connection attempt has resolved one way or another.
        let socket = self.socket.take().unwrap();
        match socket.connect_result() {
            Ok(()) => Poll::Ready(Ok(TcpStream { socket })),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// ReadFuture
// ---------------------------------------------------------------------------

pub struct ReadFuture<'a> {
    socket: &'a Socket,
    buf: &'a mut [u8],
}

impl Future for ReadFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.socket.recv(this.buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_read(this.socket.handle(), cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// WriteFuture
// ---------------------------------------------------------------------------

pub struct WriteFuture<'a> {
    socket: &'a Socket,
    buf: &'a [u8],
}

impl Future for WriteFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.socket.send(this.buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_write(this.socket.handle(), cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct WriteVectoredFuture<'a> {
    socket: &'a Socket,
    buffers: &'a [IoSlice<'a>],
}

impl Future for WriteVectoredFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.socket.send_vectored(this.buffers) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_write(this.socket.handle(), cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Wake, Waker};

    struct CountWake(AtomicUsize);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn client_server_localhost() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _peer) = listener.accept().await.unwrap();

            let mut buf = [0u8; 5];
            let n = stream.read(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], b"hello");

            stream.write(b"world").await.unwrap();
        });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write(b"hello").await.unwrap();

        let mut buf = [0u8; 5];
        let n = stream.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"world");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn dropping_stream_wakes_pending_io() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut stream, _) = listener.accept().await.unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&counter));
        let mut context = Context::from_waker(&waker);
        let mut buffer = [0; 1];
        let mut read = Box::pin(stream.read(&mut buffer));

        assert!(matches!(read.as_mut().poll(&mut context), Poll::Pending));
        drop(read);
        drop(stream);

        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        drop(peer);
    }
}
