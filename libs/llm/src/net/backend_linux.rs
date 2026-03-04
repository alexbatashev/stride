use super::stream::Event;
use std::io;
use std::os::fd::RawFd;

mod sys {
    use std::os::raw::c_int;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct EpollEvent {
        pub events: u32,
        pub data: u64,
    }

    pub const EPOLL_CLOEXEC: c_int = 0x80000;
    pub const EPOLL_CTL_ADD: c_int = 1;
    pub const EPOLL_CTL_DEL: c_int = 2;
    pub const EPOLL_CTL_MOD: c_int = 3;
    pub const EPOLLIN: u32 = 0x001;
    pub const EPOLLOUT: u32 = 0x004;
    pub const EPOLLERR: u32 = 0x008;
    pub const EPOLLHUP: u32 = 0x010;

    unsafe extern "C" {
        pub fn epoll_create1(flags: c_int) -> c_int;
        pub fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut EpollEvent) -> c_int;
        pub fn epoll_wait(
            epfd: c_int,
            events: *mut EpollEvent,
            maxevents: c_int,
            timeout: c_int,
        ) -> c_int;
        pub fn close(fd: c_int) -> c_int;
    }
}

pub(super) struct Backend {
    epoll_fd: RawFd,
}

impl Backend {
    pub(super) fn new() -> io::Result<Self> {
        let epoll_fd = unsafe { sys::epoll_create1(sys::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { epoll_fd })
    }

    pub(super) fn set_interest(&mut self, fd: RawFd, read: bool, write: bool) -> io::Result<()> {
        if !read && !write {
            return self.delete(fd);
        }

        let mut flags = sys::EPOLLERR | sys::EPOLLHUP;
        if read {
            flags |= sys::EPOLLIN;
        }
        if write {
            flags |= sys::EPOLLOUT;
        }

        let mut event = sys::EpollEvent {
            events: flags,
            data: fd as u64,
        };

        let add_rc = unsafe {
            sys::epoll_ctl(
                self.epoll_fd,
                sys::EPOLL_CTL_ADD,
                fd,
                &mut event as *mut sys::EpollEvent,
            )
        };
        if add_rc == 0 {
            return Ok(());
        }

        let mod_rc = unsafe {
            sys::epoll_ctl(
                self.epoll_fd,
                sys::EPOLL_CTL_MOD,
                fd,
                &mut event as *mut sys::EpollEvent,
            )
        };
        if mod_rc == 0 {
            return Ok(());
        }

        Err(io::Error::last_os_error())
    }

    pub(super) fn delete(&mut self, fd: RawFd) -> io::Result<()> {
        let rc =
            unsafe { sys::epoll_ctl(self.epoll_fd, sys::EPOLL_CTL_DEL, fd, std::ptr::null_mut()) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(2) | Some(9)) {
            Ok(())
        } else {
            Err(err)
        }
    }

    pub(super) fn wait(&mut self, timeout_ms: i32, out: &mut Vec<Event>) -> io::Result<()> {
        let mut events = [sys::EpollEvent { events: 0, data: 0 }; 128];
        let n = unsafe {
            sys::epoll_wait(
                self.epoll_fd,
                events.as_mut_ptr(),
                events.len() as i32,
                timeout_ms,
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
            let mask = ev.events;
            let readable = (mask & sys::EPOLLIN) != 0 || (mask & sys::EPOLLHUP) != 0;
            let writable = (mask & sys::EPOLLOUT) != 0 || (mask & sys::EPOLLERR) != 0;
            out.push(Event {
                fd: ev.data as RawFd,
                readable,
                writable,
            });
        }
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = unsafe { sys::close(self.epoll_fd) };
    }
}
