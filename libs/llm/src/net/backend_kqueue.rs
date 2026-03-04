use super::stream::Event;
use std::io;
use std::os::fd::RawFd;

mod sys {
    use std::os::raw::{c_int, c_void};

    pub type UintptrT = usize;
    pub type IntptrT = isize;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Timespec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct KEvent {
        pub ident: UintptrT,
        pub filter: i16,
        pub flags: u16,
        pub fflags: u32,
        pub data: IntptrT,
        pub udata: *mut c_void,
    }

    pub const EV_ADD: u16 = 0x0001;
    pub const EV_DELETE: u16 = 0x0002;
    pub const EV_ENABLE: u16 = 0x0004;
    pub const EV_EOF: u16 = 0x8000;
    pub const EV_ERROR: u16 = 0x4000;
    pub const EVFILT_READ: i16 = -1;
    pub const EVFILT_WRITE: i16 = -2;

    unsafe extern "C" {
        pub fn kqueue() -> c_int;
        pub fn kevent(
            kq: c_int,
            changelist: *const KEvent,
            nchanges: c_int,
            eventlist: *mut KEvent,
            nevents: c_int,
            timeout: *const Timespec,
        ) -> c_int;
        pub fn close(fd: c_int) -> c_int;
    }
}

pub(super) struct Backend {
    kqueue_fd: RawFd,
}

impl Backend {
    pub(super) fn new() -> io::Result<Self> {
        let kqueue_fd = unsafe { sys::kqueue() };
        if kqueue_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { kqueue_fd })
    }

    pub(super) fn set_interest(&mut self, fd: RawFd, read: bool, write: bool) -> io::Result<()> {
        self.update_filter(fd, sys::EVFILT_READ, read)?;
        self.update_filter(fd, sys::EVFILT_WRITE, write)?;
        Ok(())
    }

    fn update_filter(&mut self, fd: RawFd, filter: i16, enabled: bool) -> io::Result<()> {
        let mut event: sys::KEvent = unsafe { std::mem::zeroed() };
        event.ident = fd as sys::UintptrT;
        event.filter = filter;
        event.flags = if enabled {
            sys::EV_ADD | sys::EV_ENABLE
        } else {
            sys::EV_DELETE
        };
        let rc = unsafe {
            sys::kevent(
                self.kqueue_fd,
                &event as *const sys::KEvent,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if !enabled && matches!(err.raw_os_error(), Some(2) | Some(9)) {
            Ok(())
        } else {
            Err(err)
        }
    }

    pub(super) fn delete(&mut self, fd: RawFd) -> io::Result<()> {
        let _ = self.update_filter(fd, sys::EVFILT_READ, false);
        let _ = self.update_filter(fd, sys::EVFILT_WRITE, false);
        Ok(())
    }

    pub(super) fn wait(&mut self, timeout_ms: i32, out: &mut Vec<Event>) -> io::Result<()> {
        let mut events: [sys::KEvent; 128] = unsafe { std::mem::zeroed() };
        let timeout = sys::Timespec {
            tv_sec: (timeout_ms / 1000) as i64,
            tv_nsec: ((timeout_ms % 1000) * 1_000_000) as i64,
        };

        let n = unsafe {
            sys::kevent(
                self.kqueue_fd,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                &timeout as *const sys::Timespec,
            )
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(err);
        }

        for ev in events.iter().take(n as usize) {
            let is_error = (ev.flags & sys::EV_ERROR) != 0;
            let is_eof = (ev.flags & sys::EV_EOF) != 0;
            let readable = ev.filter == sys::EVFILT_READ || is_error || is_eof;
            let writable = ev.filter == sys::EVFILT_WRITE || is_error;
            out.push(Event {
                fd: ev.ident as RawFd,
                readable,
                writable,
            });
        }
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = unsafe { sys::close(self.kqueue_fd) };
    }
}
