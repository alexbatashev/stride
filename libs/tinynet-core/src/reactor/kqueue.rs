use std::{collections::HashMap, io, os::fd::RawFd, ptr, sync::Mutex, task::Waker};

use crate::sys::Handle;

/// Arbitrary non-fd identifier for the EVFILT_USER wakeup event.
const WAKEUP_IDENT: usize = 1;

#[derive(Default)]
struct WakerSlot {
    read: Option<Waker>,
    write: Option<Waker>,
}

enum Interest {
    Read(Handle),
    Write(Handle),
}

pub struct Inner {
    kqueue_fd: RawFd, // kqueue's own fd, distinct from registered socket handles
    wakers: Mutex<HashMap<Handle, WakerSlot>>,
    pending: Mutex<Vec<Interest>>,
}

impl Inner {
    pub fn new() -> io::Result<Self> {
        let kqueue_fd = unsafe { libc::kqueue() };
        if kqueue_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        unsafe { libc::fcntl(kqueue_fd, libc::F_SETFD, libc::FD_CLOEXEC) };

        // Register a persistent EVFILT_USER event used to wake the reactor
        // thread when new interests arrive while it is blocked in kevent().
        // EV_CLEAR means the trigger is consumed after each wakeup.
        let kev = make_kevent(
            WAKEUP_IDENT,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_CLEAR,
            0,
        );
        let ret = unsafe { libc::kevent(kqueue_fd, &kev, 1, ptr::null_mut(), 0, ptr::null()) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(kqueue_fd) };
            return Err(err);
        }

        Ok(Self {
            kqueue_fd,
            wakers: Mutex::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        })
    }

    pub fn register_read(&self, fd: RawFd, waker: Waker) {
        self.wakers.lock().unwrap().entry(fd).or_default().read = Some(waker);
        self.pending.lock().unwrap().push(Interest::Read(fd));
        self.trigger_wakeup();
    }

    pub fn register_write(&self, fd: RawFd, waker: Waker) {
        self.wakers.lock().unwrap().entry(fd).or_default().write = Some(waker);
        self.pending.lock().unwrap().push(Interest::Write(fd));
        self.trigger_wakeup();
    }

    pub fn deregister(&self, fd: RawFd) {
        self.pending
            .lock()
            .unwrap()
            .retain(|interest| match interest {
                Interest::Read(handle) | Interest::Write(handle) => *handle != fd,
            });
        let removed = self.wakers.lock().unwrap().remove(&fd);
        for filter in [libc::EVFILT_READ, libc::EVFILT_WRITE] {
            let event = make_kevent(fd as usize, filter, libc::EV_DELETE, 0);
            unsafe {
                libc::kevent(self.kqueue_fd, &event, 1, ptr::null_mut(), 0, ptr::null());
            }
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

    /// Interrupt a blocking kevent() call in the reactor thread so it picks
    /// up newly added interests on its next iteration.
    fn trigger_wakeup(&self) {
        let kev = make_kevent(
            WAKEUP_IDENT,
            libc::EVFILT_USER,
            libc::EV_ENABLE,
            libc::NOTE_TRIGGER,
        );
        unsafe { libc::kevent(self.kqueue_fd, &kev, 1, ptr::null_mut(), 0, ptr::null()) };
    }

    /// The reactor event loop. Runs on a dedicated background thread for the
    /// lifetime of the process.
    pub fn run_loop(&self) {
        let mut events = vec![unsafe { std::mem::zeroed::<libc::kevent>() }; 64];

        loop {
            // Take all pending interests and convert to kevent change structs.
            // The lock is held only during the drain, not during kevent().
            let changes: Vec<libc::kevent> = self
                .pending
                .lock()
                .unwrap()
                .drain(..)
                .map(|i| match i {
                    Interest::Read(fd) => make_kevent(
                        fd as usize,
                        libc::EVFILT_READ,
                        // EV_ONESHOT: auto-remove after firing so we never
                        // get stale events on subsequent polls.
                        libc::EV_ADD | libc::EV_ONESHOT,
                        0,
                    ),
                    Interest::Write(fd) => make_kevent(
                        fd as usize,
                        libc::EVFILT_WRITE,
                        libc::EV_ADD | libc::EV_ONESHOT,
                        0,
                    ),
                })
                .collect();

            // Block until at least one event fires. No locks held here.
            let n = unsafe {
                libc::kevent(
                    self.kqueue_fd,
                    if changes.is_empty() {
                        ptr::null()
                    } else {
                        changes.as_ptr()
                    },
                    changes.len() as libc::c_int,
                    events.as_mut_ptr(),
                    events.len() as libc::c_int,
                    ptr::null(), // no timeout: sleep until something happens
                )
            };

            if n < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue; // signal interrupted the syscall, retry
                }
                eprintln!("tinynet reactor: kevent() failed, shutting down reactor thread");
                break;
            }

            // Collect wakers to fire. Take them out of the map under the lock,
            // then fire them after releasing it to avoid potential re-entrant
            // locking if wake() causes an immediate re-registration.
            let mut wakers_to_fire: Vec<Waker> = Vec::new();
            {
                let mut wakers = self.wakers.lock().unwrap();
                for ev in &events[..n as usize] {
                    // EV_ERROR means the change we submitted was invalid
                    // (e.g. bad fd). Skip it; the future will surface the
                    // error when it next tries the actual I/O syscall.
                    if ev.flags & libc::EV_ERROR != 0 || ev.filter == libc::EVFILT_USER {
                        continue;
                    }
                    let fd = ev.ident as RawFd;
                    if let Some(slot) = wakers.get_mut(&fd) {
                        let taken = if ev.filter == libc::EVFILT_READ {
                            slot.read.take()
                        } else if ev.filter == libc::EVFILT_WRITE {
                            slot.write.take()
                        } else {
                            None
                        };
                        if let Some(w) = taken {
                            wakers_to_fire.push(w);
                        }
                        if slot.read.is_none() && slot.write.is_none() {
                            wakers.remove(&fd);
                        }
                    }
                }
            }

            for waker in wakers_to_fire {
                waker.wake();
            }
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe { libc::close(self.kqueue_fd) };
    }
}

fn make_kevent(ident: usize, filter: i16, flags: u16, fflags: u32) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: ptr::null_mut(),
    }
}
