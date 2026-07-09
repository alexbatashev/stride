use std::{
    sync::{Arc, LazyLock},
    task::Waker,
    time::Instant,
};

mod timer;
pub use timer::{Sleep, deadline, sleep};

#[cfg(target_os = "linux")]
mod epoll;
#[cfg(target_os = "linux")]
mod io_uring;
#[cfg(target_os = "linux")]
enum AnyInner {
    IoUring(io_uring::Inner),
    Epoll(epoll::Inner),
}

#[cfg(target_os = "linux")]
impl AnyInner {
    fn new() -> std::io::Result<Self> {
        if std::env::var_os("TINYNET_FORCE_EPOLL").is_some() {
            return epoll::Inner::new().map(Self::Epoll);
        }
        match io_uring::Inner::new() {
            Ok(inner) => Ok(Self::IoUring(inner)),
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOSYS) | Some(libc::EPERM) | Some(libc::EACCES)
                ) =>
            {
                epoll::Inner::new().map(Self::Epoll)
            }
            Err(error) => Err(error),
        }
    }

    fn register_read(&self, handle: Handle, waker: Waker) {
        match self {
            Self::IoUring(inner) => inner.register_read(handle, waker),
            Self::Epoll(inner) => inner.register_read(handle, waker),
        }
    }

    fn register_write(&self, handle: Handle, waker: Waker) {
        match self {
            Self::IoUring(inner) => inner.register_write(handle, waker),
            Self::Epoll(inner) => inner.register_write(handle, waker),
        }
    }

    fn deregister(&self, handle: Handle) {
        match self {
            Self::IoUring(inner) => inner.deregister(handle),
            Self::Epoll(inner) => inner.deregister(handle),
        }
    }

    fn run_loop(&self) {
        match self {
            Self::IoUring(inner) => inner.run_loop(),
            Self::Epoll(inner) => inner.run_loop(),
        }
    }
}

#[cfg(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
mod kqueue;
#[cfg(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
use kqueue as sys;

pub use crate::sys::Handle;

pub struct Reactor {
    #[cfg(target_os = "linux")]
    inner: Arc<AnyInner>,
    #[cfg(not(target_os = "linux"))]
    inner: Arc<sys::Inner>,
    timers: Arc<timer::TimerQueue>,
}

impl Reactor {
    fn new() -> Self {
        #[cfg(target_os = "linux")]
        let inner = Arc::new(AnyInner::new().expect("tinynet: failed to initialize reactor"));
        #[cfg(not(target_os = "linux"))]
        let inner = Arc::new(sys::Inner::new().expect("tinynet: failed to initialize reactor"));
        let timers = Arc::new(timer::TimerQueue::new());
        let bg = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("tinynet-reactor".into())
            .spawn(move || bg.run_loop())
            .expect("tinynet: failed to spawn reactor thread");
        let timer_bg = Arc::clone(&timers);
        std::thread::Builder::new()
            .name("tinynet-timer".into())
            .spawn(move || timer_bg.run_loop())
            .expect("tinynet: failed to spawn timer thread");
        Reactor { inner, timers }
    }

    pub fn register_read(&self, handle: Handle, waker: Waker) {
        self.inner.register_read(handle, waker);
    }

    pub fn register_write(&self, handle: Handle, waker: Waker) {
        self.inner.register_write(handle, waker);
    }

    pub(crate) fn deregister(&self, handle: Handle) {
        self.inner.deregister(handle);
    }

    fn register_timer(&self, deadline: Instant, id: Option<u64>, waker: Waker) -> u64 {
        self.timers.register(deadline, id, waker)
    }

    fn cancel_timer(&self, id: u64) {
        self.timers.cancel(id);
    }
}

/// Global reactor. Initialized on first use; the background thread runs for
/// the lifetime of the process.
pub static REACTOR: LazyLock<Reactor> = LazyLock::new(Reactor::new);
