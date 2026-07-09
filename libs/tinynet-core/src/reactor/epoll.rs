use std::{collections::HashMap, io, mem, os::fd::RawFd, sync::Mutex, task::Waker};

use crate::sys::Handle;

#[derive(Default)]
struct WakerSlot {
    read: Option<Waker>,
    write: Option<Waker>,
}

pub struct Inner {
    epoll_fd: RawFd,
    wake_fd: RawFd,
    wakers: Mutex<HashMap<Handle, WakerSlot>>,
}

impl Inner {
    pub fn new() -> io::Result<Self> {
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let wake_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if wake_fd < 0 {
            let error = io::Error::last_os_error();
            unsafe { libc::close(epoll_fd) };
            return Err(error);
        }

        let inner = Self {
            epoll_fd,
            wake_fd,
            wakers: Mutex::new(HashMap::new()),
        };
        if let Err(error) = inner.arm(wake_fd, libc::EPOLLIN as u32) {
            unsafe {
                libc::close(wake_fd);
                libc::close(epoll_fd);
            }
            return Err(error);
        }
        Ok(inner)
    }

    pub fn register_read(&self, fd: RawFd, waker: Waker) {
        self.register(fd, Some(waker), None);
    }

    pub fn register_write(&self, fd: RawFd, waker: Waker) {
        self.register(fd, None, Some(waker));
    }

    pub fn deregister(&self, fd: RawFd) {
        let removed = self.wakers.lock().unwrap().remove(&fd);
        unsafe {
            libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut());
        }
        if let Some(slot) = removed {
            if let Some(waker) = slot.read {
                waker.wake();
            }
            if let Some(waker) = slot.write {
                waker.wake();
            }
        }
    }

    fn register(&self, fd: RawFd, read: Option<Waker>, write: Option<Waker>) {
        let events = {
            let mut wakers = self.wakers.lock().unwrap();
            let slot = wakers.entry(fd).or_default();
            if read.is_some() {
                slot.read = read;
            }
            if write.is_some() {
                slot.write = write;
            }
            (if slot.read.is_some() {
                libc::EPOLLIN as u32
            } else {
                0
            }) | if slot.write.is_some() {
                libc::EPOLLOUT as u32
            } else {
                0
            }
        };

        if self.arm(fd, events).is_err() {
            let mut wakers = self.wakers.lock().unwrap();
            if let Some(slot) = wakers.remove(&fd) {
                if let Some(waker) = slot.read {
                    waker.wake();
                }
                if let Some(waker) = slot.write {
                    waker.wake();
                }
            }
        }
        self.trigger_wakeup();
    }

    fn arm(&self, fd: RawFd, events: u32) -> io::Result<()> {
        let mut event = libc::epoll_event {
            events: events | libc::EPOLLONESHOT as u32,
            u64: fd as u64,
        };
        let add = unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
        if add == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
        if unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_MOD, fd, &mut event) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn trigger_wakeup(&self) {
        let one = 1u64;
        unsafe {
            libc::write(
                self.wake_fd,
                &one as *const _ as *const libc::c_void,
                mem::size_of_val(&one),
            );
        }
    }

    pub fn run_loop(&self) {
        let mut events = vec![libc::epoll_event { events: 0, u64: 0 }; 64];
        loop {
            let count = unsafe {
                libc::epoll_wait(
                    self.epoll_fd,
                    events.as_mut_ptr(),
                    events.len() as libc::c_int,
                    -1,
                )
            };
            if count < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }

            let mut ready = Vec::new();
            for event in &events[..count as usize] {
                let fd = event.u64 as RawFd;
                if fd == self.wake_fd {
                    let mut value = 0u64;
                    unsafe {
                        libc::read(
                            self.wake_fd,
                            &mut value as *mut _ as *mut libc::c_void,
                            mem::size_of_val(&value),
                        );
                    }
                    let _ = self.arm(self.wake_fd, libc::EPOLLIN as u32);
                    continue;
                }

                let flags = event.events;
                let wake_both = flags & (libc::EPOLLERR | libc::EPOLLHUP) as u32 != 0;
                let mut wakers = self.wakers.lock().unwrap();
                if let Some(slot) = wakers.get_mut(&fd) {
                    if wake_both || flags & libc::EPOLLIN as u32 != 0 {
                        ready.extend(slot.read.take());
                    }
                    if wake_both || flags & libc::EPOLLOUT as u32 != 0 {
                        ready.extend(slot.write.take());
                    }
                    if slot.read.is_none() && slot.write.is_none() {
                        wakers.remove(&fd);
                    }
                }
            }
            for waker in ready.drain(..) {
                waker.wake();
            }
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.wake_fd);
            libc::close(self.epoll_fd);
        }
    }
}
