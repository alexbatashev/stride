use std::{collections::HashMap, io, mem, os::fd::RawFd, ptr, sync::Mutex, task::Waker};

use crate::sys::Handle;

const IORING_OFF_SQ_RING: libc::off_t = 0;
const IORING_OFF_CQ_RING: libc::off_t = 0x8000000;
const IORING_OFF_SQES: libc::off_t = 0x10000000;
const IORING_ENTER_GETEVENTS: u32 = 1;
const IORING_OP_POLL_ADD: u8 = 6;
const IORING_OP_POLL_REMOVE: u8 = 7;

const TAG_READ: u64 = 0;
const TAG_WRITE: u64 = 1;
const TAG_WAKE: u64 = 2;
const TAG_CANCEL: u64 = 3;

#[repr(C)]
#[derive(Copy, Clone)]
struct IoSqringOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    flags: u32,
    dropped: u32,
    array: u32,
    resv1: u32,
    resv2: u64,
}
#[repr(C)]
#[derive(Copy, Clone)]
struct IoCqringOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    overflow: u32,
    cqes: u32,
    flags: u32,
    resv1: u32,
    resv2: u64,
}
#[repr(C)]
struct IoUringParams {
    sq_entries: u32,
    cq_entries: u32,
    flags: u32,
    sq_thread_cpu: u32,
    sq_thread_idle: u32,
    features: u32,
    wq_fd: u32,
    resv: [u32; 3],
    sq_off: IoSqringOffsets,
    cq_off: IoCqringOffsets,
}
#[repr(C)]
struct IoUringSqe {
    opcode: u8,
    flags: u8,
    ioprio: u16,
    fd: i32,
    off: u64,
    addr: u64,
    len: u32,
    poll32_events: u32,
    user_data: u64,
    buf_index: u16,
    personality: u16,
    splice_fd_in: i32,
    pad2: [u64; 2],
}
#[repr(C)]
struct IoUringCqe {
    user_data: u64,
    res: i32,
    flags: u32,
}

enum Interest {
    Read(Handle),
    Write(Handle),
    Remove(Handle),
}
#[derive(Default)]
struct WakerSlot {
    read: Option<Waker>,
    write: Option<Waker>,
}
struct SubmissionQueue {
    khead: *mut u32,
    ktail: *mut u32,
    ring_mask: *mut u32,
    ring_entries: *mut u32,
    array: *mut u32,
    sqes: *mut IoUringSqe,
}
struct CompletionQueue {
    khead: *mut u32,
    ktail: *mut u32,
    ring_mask: *mut u32,
    cqes: *mut IoUringCqe,
}
struct Ring {
    fd: RawFd,
    wake_fd: RawFd,
    sq: SubmissionQueue,
    cq: CompletionQueue,
    sq_ring_ptr: *mut libc::c_void,
    sq_ring_sz: usize,
    cq_ring_ptr: *mut libc::c_void,
    cq_ring_sz: usize,
    sqes_ptr: *mut libc::c_void,
    sqes_sz: usize,
}
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

pub struct Inner {
    ring: Ring,
    wakers: Mutex<HashMap<Handle, WakerSlot>>,
    pending: Mutex<Vec<Interest>>,
}

impl Inner {
    pub fn new() -> io::Result<Self> {
        let mut params: IoUringParams = unsafe { mem::zeroed() };
        let fd = unsafe { libc::syscall(libc::SYS_io_uring_setup, 256u32, &mut params) as RawFd };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let sq_ring_sz =
            params.sq_off.array as usize + params.sq_entries as usize * mem::size_of::<u32>();
        let cq_ring_sz =
            params.cq_off.cqes as usize + params.cq_entries as usize * mem::size_of::<IoUringCqe>();
        let sqes_sz = params.sq_entries as usize * mem::size_of::<IoUringSqe>();

        unsafe {
            let sq_ring_ptr = libc::mmap(
                ptr::null_mut(),
                sq_ring_sz,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                IORING_OFF_SQ_RING,
            );
            if sq_ring_ptr == libc::MAP_FAILED {
                libc::close(fd);
                return Err(io::Error::last_os_error());
            }

            let cq_ring_ptr = libc::mmap(
                ptr::null_mut(),
                cq_ring_sz,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                IORING_OFF_CQ_RING,
            );
            if cq_ring_ptr == libc::MAP_FAILED {
                libc::munmap(sq_ring_ptr, sq_ring_sz);
                libc::close(fd);
                return Err(io::Error::last_os_error());
            }

            let sqes_ptr = libc::mmap(
                ptr::null_mut(),
                sqes_sz,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                IORING_OFF_SQES,
            );
            if sqes_ptr == libc::MAP_FAILED {
                libc::munmap(cq_ring_ptr, cq_ring_sz);
                libc::munmap(sq_ring_ptr, sq_ring_sz);
                libc::close(fd);
                return Err(io::Error::last_os_error());
            }

            let wake_fd = libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC);
            if wake_fd < 0 {
                libc::munmap(sqes_ptr, sqes_sz);
                libc::munmap(cq_ring_ptr, cq_ring_sz);
                libc::munmap(sq_ring_ptr, sq_ring_sz);
                libc::close(fd);
                return Err(io::Error::last_os_error());
            }

            let inner = Self {
                ring: Ring {
                    fd,
                    wake_fd,
                    sq: SubmissionQueue {
                        khead: (sq_ring_ptr as *mut u8).add(params.sq_off.head as usize)
                            as *mut u32,
                        ktail: (sq_ring_ptr as *mut u8).add(params.sq_off.tail as usize)
                            as *mut u32,
                        ring_mask: (sq_ring_ptr as *mut u8).add(params.sq_off.ring_mask as usize)
                            as *mut u32,
                        ring_entries: (sq_ring_ptr as *mut u8)
                            .add(params.sq_off.ring_entries as usize)
                            as *mut u32,
                        array: (sq_ring_ptr as *mut u8).add(params.sq_off.array as usize)
                            as *mut u32,
                        sqes: sqes_ptr as *mut IoUringSqe,
                    },
                    cq: CompletionQueue {
                        khead: (cq_ring_ptr as *mut u8).add(params.cq_off.head as usize)
                            as *mut u32,
                        ktail: (cq_ring_ptr as *mut u8).add(params.cq_off.tail as usize)
                            as *mut u32,
                        ring_mask: (cq_ring_ptr as *mut u8).add(params.cq_off.ring_mask as usize)
                            as *mut u32,
                        cqes: (cq_ring_ptr as *mut u8).add(params.cq_off.cqes as usize)
                            as *mut IoUringCqe,
                    },
                    sq_ring_ptr,
                    sq_ring_sz,
                    cq_ring_ptr,
                    cq_ring_sz,
                    sqes_ptr,
                    sqes_sz,
                },
                wakers: Mutex::new(HashMap::new()),
                pending: Mutex::new(Vec::new()),
            };

            inner.submit_poll(inner.ring.wake_fd, libc::POLLIN as u32, TAG_WAKE)?;
            Ok(inner)
        }
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
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|interest| match interest {
            Interest::Read(handle) | Interest::Write(handle) => *handle != fd,
            Interest::Remove(_) => true,
        });
        pending.push(Interest::Remove(fd));
        drop(pending);
        let removed = self.wakers.lock().unwrap().remove(&fd);
        if let Some(slot) = removed {
            if let Some(waker) = slot.read {
                waker.wake();
            }
            if let Some(waker) = slot.write {
                waker.wake();
            }
        }
        self.trigger_wakeup();
    }

    pub fn run_loop(&self) {
        loop {
            self.submit_pending();
            let ret = unsafe {
                libc::syscall(
                    libc::SYS_io_uring_enter,
                    self.ring.fd,
                    0u32,
                    1u32,
                    IORING_ENTER_GETEVENTS,
                    ptr::null::<libc::sigset_t>(),
                )
            };
            if ret < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            self.drain_completions();
        }
    }

    fn submit_pending(&self) {
        let pending: Vec<Interest> = self.pending.lock().unwrap().drain(..).collect();
        for interest in pending {
            match interest {
                Interest::Read(fd) => {
                    if self.submit_poll(fd, libc::POLLIN as u32, TAG_READ).is_err() {
                        return;
                    }
                }
                Interest::Write(fd) => {
                    if self
                        .submit_poll(fd, libc::POLLOUT as u32, TAG_WRITE)
                        .is_err()
                    {
                        return;
                    }
                }
                Interest::Remove(fd) => {
                    let _ = self.submit_remove(fd, TAG_READ);
                    let _ = self.submit_remove(fd, TAG_WRITE);
                }
            }
        }
    }

    fn trigger_wakeup(&self) {
        let one: u64 = 1;
        unsafe {
            libc::write(
                self.ring.wake_fd,
                &one as *const _ as *const libc::c_void,
                mem::size_of::<u64>(),
            );
        }
    }

    fn submit_poll(&self, fd: RawFd, events: u32, tag: u64) -> io::Result<()> {
        unsafe {
            let sq = &self.ring.sq;
            let head = ptr::read_volatile(sq.khead);
            let tail = ptr::read_volatile(sq.ktail);
            if tail.wrapping_sub(head) >= ptr::read_volatile(sq.ring_entries) {
                return Ok(());
            }
            let idx = tail & ptr::read_volatile(sq.ring_mask);
            let sqe = sq.sqes.add(idx as usize);
            ptr::write_bytes(sqe as *mut u8, 0, mem::size_of::<IoUringSqe>());
            (*sqe).opcode = IORING_OP_POLL_ADD;
            (*sqe).fd = fd;
            (*sqe).poll32_events = events;
            (*sqe).user_data = ((fd as u64) << 2) | tag;
            *sq.array.add(idx as usize) = idx;
            ptr::write_volatile(sq.ktail, tail.wrapping_add(1));
            let ret = libc::syscall(
                libc::SYS_io_uring_enter,
                self.ring.fd,
                1u32,
                0u32,
                0u32,
                ptr::null::<libc::sigset_t>(),
            );
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn submit_remove(&self, fd: RawFd, tag: u64) -> io::Result<()> {
        unsafe {
            let sq = &self.ring.sq;
            let head = ptr::read_volatile(sq.khead);
            let tail = ptr::read_volatile(sq.ktail);
            if tail.wrapping_sub(head) >= ptr::read_volatile(sq.ring_entries) {
                return Ok(());
            }
            let idx = tail & ptr::read_volatile(sq.ring_mask);
            let sqe = sq.sqes.add(idx as usize);
            ptr::write_bytes(sqe as *mut u8, 0, mem::size_of::<IoUringSqe>());
            (*sqe).opcode = IORING_OP_POLL_REMOVE;
            (*sqe).addr = ((fd as u64) << 2) | tag;
            (*sqe).user_data = ((fd as u64) << 2) | TAG_CANCEL;
            *sq.array.add(idx as usize) = idx;
            ptr::write_volatile(sq.ktail, tail.wrapping_add(1));
            let ret = libc::syscall(
                libc::SYS_io_uring_enter,
                self.ring.fd,
                1u32,
                0u32,
                0u32,
                ptr::null::<libc::sigset_t>(),
            );
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn drain_completions(&self) {
        let mut to_wake = Vec::new();
        unsafe {
            let cq = &self.ring.cq;
            let head = ptr::read_volatile(cq.khead);
            let tail = ptr::read_volatile(cq.ktail);
            let mask = ptr::read_volatile(cq.ring_mask);
            let mut count = 0u32;
            let mut wakers = self.wakers.lock().unwrap();
            while count < tail.wrapping_sub(head) {
                let idx = (head.wrapping_add(count)) & mask;
                let cqe = &*cq.cqes.add(idx as usize);
                let tag = cqe.user_data & 0b11;
                let fd = (cqe.user_data >> 2) as RawFd;

                if tag == TAG_WAKE {
                    let mut val: u64 = 0;
                    libc::read(
                        self.ring.wake_fd,
                        &mut val as *mut _ as *mut libc::c_void,
                        mem::size_of::<u64>(),
                    );
                    let _ = self.submit_poll(self.ring.wake_fd, libc::POLLIN as u32, TAG_WAKE);
                    count = count.wrapping_add(1);
                    continue;
                }

                if tag == TAG_CANCEL {
                    count = count.wrapping_add(1);
                    continue;
                }

                if let Some(slot) = wakers.get_mut(&fd) {
                    let taken = if tag == TAG_WRITE {
                        slot.write.take()
                    } else {
                        slot.read.take()
                    };
                    if let Some(w) = taken {
                        to_wake.push(w);
                    }
                    if slot.read.is_none() && slot.write.is_none() {
                        wakers.remove(&fd);
                    }
                }
                count = count.wrapping_add(1);
            }
            ptr::write_volatile(cq.khead, head.wrapping_add(count));
        }

        for w in to_wake {
            w.wake();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.ring.wake_fd);
            libc::munmap(self.ring.sq_ring_ptr, self.ring.sq_ring_sz);
            libc::munmap(self.ring.cq_ring_ptr, self.ring.cq_ring_sz);
            libc::munmap(self.ring.sqes_ptr, self.ring.sqes_sz);
            libc::close(self.ring.fd);
        }
    }
}
