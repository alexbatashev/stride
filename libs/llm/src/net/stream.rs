use std::io::{self, Read as IoRead, Write as IoWrite};
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::rt::{Read, ReadBufCursor, Write};
use mio::net::TcpStream;
use rustls::crypto::aws_lc_rs::default_provider;
use rustls::{ClientConfig, ClientConnection, RootCertStore, pki_types::ServerName};
use rustls_aws_lc_rs as provider;

pub struct AsyncTlsStream {
    socket: TcpStream,
    config: Arc<ClientConfig>,
    tls_conn: ClientConnection,
}

pub struct AsyncTcpStream(TcpStream);

impl AsyncTlsStream {
    pub fn connect(addr: SocketAddr, server_name: &str) -> std::io::Result<Self> {
        let socket = TcpStream::connect(addr)?;
        let server_name = ServerName::try_from(server_name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?
            .to_owned();
        let config = make_config();
        let tls_conn = ClientConnection::new(config.clone(), server_name)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        // FIXME: mio docs say I need to wait for a (writable) event and check TcpStream::take_err?

        Ok(Self {
            socket,
            config,
            tls_conn,
        })
    }
}

impl Read for AsyncTlsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();

        // Read TLS bytes from the socket into the TLS session
        match this.tls_conn.read_tls(&mut this.socket) {
            Err(e) if !is_pending_socket_error(&e) => return Poll::Ready(Err(e)),
            Err(_) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Ok(_) => {}
        }

        // Process any newly received TLS messages
        let io_state = match this.tls_conn.process_new_packets() {
            Ok(state) => state,
            Err(e) => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    e.to_string(),
                )))
            }
        };

        // Read available plaintext into the output buffer
        let plaintext_len = io_state.plaintext_bytes_to_read();
        if plaintext_len > 0 {
            // Safety: we write exactly `to_read` bytes before advancing the cursor
            let dst = unsafe { buf.as_mut() };
            let to_read = plaintext_len.min(dst.len());
            let dst_slice = unsafe {
                std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u8, to_read)
            };
            let n = this.tls_conn.reader().read(dst_slice)?;
            unsafe { buf.advance(n) };
            return Poll::Ready(Ok(()));
        }

        if io_state.peer_has_closed() {
            return Poll::Ready(Ok(()));
        }

        // No plaintext yet; re-schedule and pend
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Write for AsyncTlsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        // Write plaintext into the TLS session buffer; actual socket write happens in flush
        let n = this.tls_conn.writer().write(buf)?;
        // Best-effort flush to avoid buffering too much
        let _ = this.tls_conn.write_tls(&mut this.socket);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        while this.tls_conn.wants_write() {
            match this.tls_conn.write_tls(&mut this.socket) {
                Ok(_) => {}
                Err(e) if is_pending_socket_error(&e) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        this.tls_conn.send_close_notify();
        while this.tls_conn.wants_write() {
            match this.tls_conn.write_tls(&mut this.socket) {
                Ok(_) => {}
                Err(e) if is_pending_socket_error(&e) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncTcpStream {
    pub async fn new(addr: SocketAddr) -> std::io::Result<Self> {
        let conn = TcpStream::connect(addr)?;

        // FIXME: mio docs say I need to wait for a (writable) event and check TcpStream::take_err?

        Ok(Self(conn))
    }
}

impl Read for AsyncTcpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        // Safety: we write exactly `n` bytes before advancing the cursor
        let dst = unsafe { buf.as_mut() };
        let dst_slice = unsafe {
            std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u8, dst.len())
        };
        match self.get_mut().0.read(dst_slice) {
            Ok(n) => {
                unsafe { buf.advance(n) };
                Poll::Ready(Ok(()))
            }
            Err(e) if is_pending_socket_error(&e) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl Write for AsyncTcpStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut().0.write(buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if is_pending_socket_error(&e) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(self.get_mut().0.flush())
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(self.get_mut().0.shutdown(std::net::Shutdown::Write))
    }
}

fn make_config() -> Arc<ClientConfig> {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .expect("default TLS versions are valid")
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    )
}

fn is_pending_socket_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected | io::ErrorKind::Interrupted
    )
}
