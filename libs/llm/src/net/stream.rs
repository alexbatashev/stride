use hyper::rt::Read;
use hyper::rt::Write;
use std::collections::HashMap;
use std::io::{self, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread;

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
use super::backend_fallback::Backend;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
use super::backend_kqueue::Backend;
#[cfg(target_os = "linux")]
use super::backend_linux::Backend;

pub struct AsyncTcpStream(TcpStream);

impl AsyncTcpStream {
    pub fn connect(addr: SocketAddr) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }

    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }

    pub fn into_inner(self) -> TcpStream {
        let this = std::mem::ManuallyDrop::new(self);
        unsafe { std::ptr::read(&this.0) }
    }
}

impl Drop for AsyncTcpStream {
    fn drop(&mut self) {
        if let Ok(r) = reactor() {
            let _ = r.deregister(self.0.as_raw_fd());
        }
    }
}

impl Read for AsyncTcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        let available = buf.remaining();
        if available == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut tmp = [0u8; 8 * 1024];
        let read_len = available.min(tmp.len());
        loop {
            match this.0.read(&mut tmp[..read_len]) {
                Ok(0) => return Poll::Ready(Ok(())),
                Ok(n) => {
                    buf.put_slice(&tmp[..n]);
                    return Poll::Ready(Ok(()));
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    let fd = this.0.as_raw_fd();
                    match reactor().and_then(|r| r.arm(fd, Interest::Read, cx.waker().clone())) {
                        Ok(()) => return Poll::Pending,
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}

impl Write for AsyncTcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        loop {
            match this.0.write(buf) {
                Ok(n) => return Poll::Ready(Ok(n)),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    let fd = this.0.as_raw_fd();
                    match reactor().and_then(|r| r.arm(fd, Interest::Write, cx.waker().clone())) {
                        Ok(()) => return Poll::Pending,
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        loop {
            match this.0.flush() {
                Ok(()) => return Poll::Ready(Ok(())),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    let fd = this.0.as_raw_fd();
                    match reactor().and_then(|r| r.arm(fd, Interest::Write, cx.waker().clone())) {
                        Ok(()) => return Poll::Pending,
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(self.get_mut().0.shutdown(Shutdown::Write))
    }
}

#[derive(Clone, Copy)]
enum Interest {
    Read,
    Write,
}

#[derive(Default)]
struct Entry {
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    read_armed: bool,
    write_armed: bool,
}

enum Command {
    Arm {
        fd: RawFd,
        interest: Interest,
        waker: Waker,
    },
    Deregister {
        fd: RawFd,
    },
}

struct Reactor {
    tx: mpsc::Sender<Command>,
}

impl Reactor {
    fn start() -> io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Command>();
        let mut backend = Backend::new()?;
        thread::Builder::new()
            .name("llm-net-reactor".to_owned())
            .spawn(move || run_reactor(rx, &mut backend))
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { tx })
    }

    fn arm(&self, fd: RawFd, interest: Interest, waker: Waker) -> io::Result<()> {
        self.tx
            .send(Command::Arm {
                fd,
                interest,
                waker,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "reactor thread stopped"))
    }

    fn deregister(&self, fd: RawFd) -> io::Result<()> {
        self.tx
            .send(Command::Deregister { fd })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "reactor thread stopped"))
    }
}

fn run_reactor(rx: mpsc::Receiver<Command>, backend: &mut Backend) {
    let mut entries: HashMap<RawFd, Entry> = HashMap::new();
    let mut events = Vec::with_capacity(128);

    while drain_commands(&rx, backend, &mut entries) {
        events.clear();
        if backend.wait(100, &mut events).is_err() {
            break;
        }
        for event in &events {
            let mut remove = false;
            if let Some(entry) = entries.get_mut(&event.fd) {
                if event.readable {
                    if let Some(w) = entry.read_waker.take() {
                        w.wake();
                    }
                }
                if event.writable {
                    if let Some(w) = entry.write_waker.take() {
                        w.wake();
                    }
                }

                let next_read = entry.read_waker.is_some();
                let next_write = entry.write_waker.is_some();

                if next_read != entry.read_armed || next_write != entry.write_armed {
                    entry.read_armed = next_read;
                    entry.write_armed = next_write;
                    let _ = backend.set_interest(event.fd, next_read, next_write);
                }

                if !next_read && !next_write {
                    remove = true;
                }
            }
            if remove {
                entries.remove(&event.fd);
                let _ = backend.delete(event.fd);
            }
        }
    }
}

fn drain_commands(
    rx: &mpsc::Receiver<Command>,
    backend: &mut Backend,
    entries: &mut HashMap<RawFd, Entry>,
) -> bool {
    loop {
        match rx.try_recv() {
            Ok(Command::Arm {
                fd,
                interest,
                waker,
            }) => {
                let entry = entries.entry(fd).or_default();
                match interest {
                    Interest::Read => replace_waker(&mut entry.read_waker, waker),
                    Interest::Write => replace_waker(&mut entry.write_waker, waker),
                }

                let next_read = entry.read_waker.is_some();
                let next_write = entry.write_waker.is_some();

                if next_read != entry.read_armed || next_write != entry.write_armed {
                    entry.read_armed = next_read;
                    entry.write_armed = next_write;
                    if backend.set_interest(fd, next_read, next_write).is_err() {
                        entries.remove(&fd);
                    }
                }
            }
            Ok(Command::Deregister { fd }) => {
                entries.remove(&fd);
                let _ = backend.delete(fd);
            }
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}

fn replace_waker(slot: &mut Option<Waker>, new_waker: Waker) {
    match slot {
        Some(existing) if existing.will_wake(&new_waker) => {}
        _ => *slot = Some(new_waker),
    }
}

static REACTOR: OnceLock<io::Result<Reactor>> = OnceLock::new();

fn reactor() -> io::Result<&'static Reactor> {
    match REACTOR.get_or_init(Reactor::start) {
        Ok(r) => Ok(r),
        Err(err) => Err(io::Error::new(err.kind(), err.to_string())),
    }
}

pub struct Event {
    pub fd: RawFd,
    pub readable: bool,
    pub writable: bool,
}
