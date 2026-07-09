use std::{
    io::{self, IoSlice},
    mem,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
};

pub type Handle = RawFd;

pub struct Socket(OwnedFd);

impl Drop for Socket {
    fn drop(&mut self) {
        crate::reactor::REACTOR.deregister(self.0.as_raw_fd());
    }
}

impl Socket {
    pub fn new_tcp(addr: SocketAddr) -> io::Result<Self> {
        let family = match addr {
            SocketAddr::V4(_) => libc::AF_INET,
            SocketAddr::V6(_) => libc::AF_INET6,
        };
        let raw = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a valid, freshly-created fd with no other owner.
        let socket = Self(unsafe { OwnedFd::from_raw_fd(raw) });

        let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) };

        // Deliver EPIPE instead of raising SIGPIPE when writing to a
        // half-closed connection. macOS equivalent of Linux's MSG_NOSIGNAL.
        #[cfg(target_vendor = "apple")]
        {
            let one: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    raw,
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    &one as *const _ as *const libc::c_void,
                    mem::size_of_val(&one) as libc::socklen_t,
                )
            };
        }

        Ok(socket)
    }

    pub fn new_udp(addr: SocketAddr) -> io::Result<Self> {
        let family = match addr {
            SocketAddr::V4(_) => libc::AF_INET,
            SocketAddr::V6(_) => libc::AF_INET6,
        };
        let raw = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let socket = Self(unsafe { OwnedFd::from_raw_fd(raw) });
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) };
        Ok(socket)
    }

    /// Start a non-blocking connect. Returns `Ok(())` on immediate success
    /// (rare, e.g. loopback) or `Err(WouldBlock)` when the connection is in
    /// progress. Any other error is fatal.
    pub fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        let fd = self.0.as_raw_fd();
        let ret = match addr {
            SocketAddr::V4(v4) => {
                let mut sa: libc::sockaddr_in = unsafe { mem::zeroed() };
                sa.sin_family = libc::AF_INET as libc::sa_family_t;
                sa.sin_port = v4.port().to_be();
                // octets() is already network-byte-order; from_ne_bytes keeps
                // the exact bytes intact on any host endianness.
                sa.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
                #[cfg(target_vendor = "apple")]
                {
                    sa.sin_len = mem::size_of_val(&sa) as u8;
                }
                unsafe {
                    libc::connect(
                        fd,
                        &sa as *const _ as *const libc::sockaddr,
                        mem::size_of_val(&sa) as libc::socklen_t,
                    )
                }
            }
            SocketAddr::V6(v6) => {
                let mut sa: libc::sockaddr_in6 = unsafe { mem::zeroed() };
                sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sa.sin6_port = v6.port().to_be();
                sa.sin6_flowinfo = v6.flowinfo().to_be();
                sa.sin6_addr.s6_addr = v6.ip().octets();
                sa.sin6_scope_id = v6.scope_id();
                #[cfg(target_vendor = "apple")]
                {
                    sa.sin6_len = mem::size_of_val(&sa) as u8;
                }
                unsafe {
                    libc::connect(
                        fd,
                        &sa as *const _ as *const libc::sockaddr,
                        mem::size_of_val(&sa) as libc::socklen_t,
                    )
                }
            }
        };

        if ret == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        // EINPROGRESS is not an error — the connection is underway.
        // Normalise to WouldBlock so callers need no OS-specific knowledge.
        if err.raw_os_error() == Some(libc::EINPROGRESS) {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        } else {
            Err(err)
        }
    }

    /// Check whether a previously-initiated connect completed successfully.
    /// Call this after the reactor signals write-readiness.
    pub fn connect_result(&self) -> io::Result<()> {
        let mut err: libc::c_int = 0;
        let mut len = mem::size_of_val(&err) as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                self.0.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut err as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if err == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(err))
        }
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::recv(
                self.0.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        // MSG_NOSIGNAL is Linux-only; macOS uses SO_NOSIGPIPE (set in new_tcp).
        #[cfg(target_os = "linux")]
        let flags = libc::MSG_NOSIGNAL;
        #[cfg(not(target_os = "linux"))]
        let flags = 0;

        let n = unsafe {
            libc::send(
                self.0.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
                flags,
            )
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }

    pub fn send_vectored(&self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        let sent = unsafe {
            libc::writev(
                self.0.as_raw_fd(),
                buffers.as_ptr() as *const libc::iovec,
                buffers.len() as libc::c_int,
            )
        };
        if sent < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(sent as usize)
        }
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut len = mem::size_of_val(&storage) as libc::socklen_t;
        let received = unsafe {
            libc::recvfrom(
                self.0.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut storage as *mut _ as *mut libc::sockaddr,
                &mut len,
            )
        };
        if received < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok((received as usize, sockaddr_to_addr(&storage, len)?))
        }
    }

    pub fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let (storage, len) = socket_addr_to_storage(addr);
        let sent = unsafe {
            libc::sendto(
                self.0.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
                0,
                &storage as *const _ as *const libc::sockaddr,
                len,
            )
        };
        if sent < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(sent as usize)
        }
    }

    pub fn send_batch(&self, packets: &[(&[u8], SocketAddr)]) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            let addresses: Vec<_> = packets
                .iter()
                .map(|(_, addr)| socket_addr_to_storage(*addr))
                .collect();
            let mut vectors: Vec<_> = packets
                .iter()
                .map(|(buffer, _)| libc::iovec {
                    iov_base: buffer.as_ptr() as *mut libc::c_void,
                    iov_len: buffer.len(),
                })
                .collect();
            let mut messages: Vec<libc::mmsghdr> = addresses
                .iter()
                .zip(vectors.iter_mut())
                .map(|((storage, len), vector)| libc::mmsghdr {
                    msg_hdr: libc::msghdr {
                        msg_name: storage as *const _ as *mut libc::c_void,
                        msg_namelen: *len,
                        msg_iov: vector,
                        msg_iovlen: 1,
                        msg_control: std::ptr::null_mut(),
                        msg_controllen: 0,
                        msg_flags: 0,
                    },
                    msg_len: 0,
                })
                .collect();
            let sent = unsafe {
                libc::sendmmsg(
                    self.0.as_raw_fd(),
                    messages.as_mut_ptr(),
                    messages.len() as libc::c_uint,
                    libc::MSG_NOSIGNAL,
                )
            };
            if sent < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(sent as usize)
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut sent = 0;
            for (buffer, addr) in packets {
                match self.send_to(buffer, *addr) {
                    Ok(_) => sent += 1,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock && sent > 0 => break,
                    Err(error) => return Err(error),
                }
            }
            Ok(sent)
        }
    }

    pub fn recv_batch(&self, buffers: &mut [&mut [u8]]) -> io::Result<Vec<(usize, SocketAddr)>> {
        #[cfg(target_os = "linux")]
        {
            let mut addresses: Vec<libc::sockaddr_storage> = (0..buffers.len())
                .map(|_| unsafe { mem::zeroed() })
                .collect();
            let mut vectors: Vec<_> = buffers
                .iter_mut()
                .map(|buffer| libc::iovec {
                    iov_base: buffer.as_mut_ptr() as *mut libc::c_void,
                    iov_len: buffer.len(),
                })
                .collect();
            let mut messages: Vec<libc::mmsghdr> = addresses
                .iter_mut()
                .zip(vectors.iter_mut())
                .map(|(storage, vector)| libc::mmsghdr {
                    msg_hdr: libc::msghdr {
                        msg_name: storage as *mut _ as *mut libc::c_void,
                        msg_namelen: mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t,
                        msg_iov: vector,
                        msg_iovlen: 1,
                        msg_control: std::ptr::null_mut(),
                        msg_controllen: 0,
                        msg_flags: 0,
                    },
                    msg_len: 0,
                })
                .collect();
            let received = unsafe {
                libc::recvmmsg(
                    self.0.as_raw_fd(),
                    messages.as_mut_ptr(),
                    messages.len() as libc::c_uint,
                    libc::MSG_DONTWAIT,
                    std::ptr::null_mut(),
                )
            };
            if received < 0 {
                return Err(io::Error::last_os_error());
            }
            messages
                .iter()
                .zip(addresses.iter())
                .take(received as usize)
                .map(|(message, storage)| {
                    Ok((
                        message.msg_len as usize,
                        sockaddr_to_addr(storage, message.msg_hdr.msg_namelen)?,
                    ))
                })
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut received = Vec::new();
            for buffer in buffers {
                match self.recv_from(buffer) {
                    Ok(packet) => received.push(packet),
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock && !received.is_empty() =>
                    {
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(received)
        }
    }

    pub fn bind(&self, addr: SocketAddr) -> io::Result<()> {
        let fd = self.0.as_raw_fd();

        // Allow fast server restarts without waiting out TIME_WAIT.
        let one: libc::c_int = 1;
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &one as *const _ as *const libc::c_void,
                mem::size_of_val(&one) as libc::socklen_t,
            )
        };

        let ret = match addr {
            SocketAddr::V4(v4) => {
                let mut sa: libc::sockaddr_in = unsafe { mem::zeroed() };
                sa.sin_family = libc::AF_INET as libc::sa_family_t;
                sa.sin_port = v4.port().to_be();
                sa.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
                #[cfg(target_vendor = "apple")]
                {
                    sa.sin_len = mem::size_of_val(&sa) as u8;
                }
                unsafe {
                    libc::bind(
                        fd,
                        &sa as *const _ as *const libc::sockaddr,
                        mem::size_of_val(&sa) as libc::socklen_t,
                    )
                }
            }
            SocketAddr::V6(v6) => {
                let mut sa: libc::sockaddr_in6 = unsafe { mem::zeroed() };
                sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sa.sin6_port = v6.port().to_be();
                sa.sin6_flowinfo = v6.flowinfo().to_be();
                sa.sin6_addr.s6_addr = v6.ip().octets();
                sa.sin6_scope_id = v6.scope_id();
                #[cfg(target_vendor = "apple")]
                {
                    sa.sin6_len = mem::size_of_val(&sa) as u8;
                }
                unsafe {
                    libc::bind(
                        fd,
                        &sa as *const _ as *const libc::sockaddr,
                        mem::size_of_val(&sa) as libc::socklen_t,
                    )
                }
            }
        };
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn listen(&self, backlog: i32) -> io::Result<()> {
        if unsafe { libc::listen(self.0.as_raw_fd(), backlog) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Non-blocking accept. Returns `Err(WouldBlock)` when no connections are
    /// queued; the caller should register read interest and retry.
    pub fn accept(&self) -> io::Result<(Socket, SocketAddr)> {
        let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut len = mem::size_of_val(&storage) as libc::socklen_t;
        let fd = unsafe {
            libc::accept(
                self.0.as_raw_fd(),
                &mut storage as *mut _ as *mut libc::sockaddr,
                &mut len,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: accept() returned a valid fd with no other owner.
        let socket = Socket(unsafe { OwnedFd::from_raw_fd(fd) });

        // The accepted socket does not inherit O_NONBLOCK or FD_CLOEXEC.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };

        #[cfg(target_vendor = "apple")]
        {
            let one: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    &one as *const _ as *const libc::c_void,
                    mem::size_of_val(&one) as libc::socklen_t,
                )
            };
        }

        let addr = sockaddr_to_addr(&storage, len)?;
        Ok((socket, addr))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut len = mem::size_of_val(&storage) as libc::socklen_t;
        if unsafe {
            libc::getsockname(
                self.0.as_raw_fd(),
                &mut storage as *mut _ as *mut libc::sockaddr,
                &mut len,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        sockaddr_to_addr(&storage, len)
    }

    pub fn handle(&self) -> Handle {
        self.0.as_raw_fd()
    }
}

fn socket_addr_to_storage(addr: SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
    match addr {
        SocketAddr::V4(v4) => {
            let target = unsafe { &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in) };
            target.sin_family = libc::AF_INET as libc::sa_family_t;
            target.sin_port = v4.port().to_be();
            target.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
            #[cfg(target_vendor = "apple")]
            {
                target.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(v6) => {
            let target = unsafe { &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in6) };
            target.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            target.sin6_port = v6.port().to_be();
            target.sin6_flowinfo = v6.flowinfo().to_be();
            target.sin6_addr.s6_addr = v6.ip().octets();
            target.sin6_scope_id = v6.scope_id();
            #[cfg(target_vendor = "apple")]
            {
                target.sin6_len = mem::size_of::<libc::sockaddr_in6>() as u8;
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    }
}

fn sockaddr_to_addr(
    storage: &libc::sockaddr_storage,
    _len: libc::socklen_t,
) -> io::Result<SocketAddr> {
    match storage.ss_family as libc::c_int {
        libc::AF_INET => {
            // SAFETY: ss_family confirms this is a sockaddr_in.
            let sa = unsafe { &*(storage as *const _ as *const libc::sockaddr_in) };
            // s_addr bytes are in network order; to_ne_bytes preserves them
            // as octets on any host endianness (inverse of from_ne_bytes).
            let ip = Ipv4Addr::from(sa.sin_addr.s_addr.to_ne_bytes());
            let port = u16::from_be(sa.sin_port);
            Ok(SocketAddr::V4(SocketAddrV4::new(ip, port)))
        }
        libc::AF_INET6 => {
            // SAFETY: ss_family confirms this is a sockaddr_in6.
            let sa = unsafe { &*(storage as *const _ as *const libc::sockaddr_in6) };
            let ip = Ipv6Addr::from(sa.sin6_addr.s6_addr);
            let port = u16::from_be(sa.sin6_port);
            Ok(SocketAddr::V6(SocketAddrV6::new(
                ip,
                port,
                u32::from_be(sa.sin6_flowinfo),
                sa.sin6_scope_id,
            )))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported address family",
        )),
    }
}
