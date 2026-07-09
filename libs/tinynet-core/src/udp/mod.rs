use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{reactor::REACTOR, sys::Socket};

pub struct UdpSocket {
    socket: Socket,
}

impl UdpSocket {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        let socket = Socket::new_udp(addr)?;
        socket.bind(addr)?;
        Ok(Self { socket })
    }

    pub fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        self.socket.connect(addr)
    }

    pub fn send<'a>(&'a self, buffer: &'a [u8]) -> SendFuture<'a> {
        SendFuture {
            socket: &self.socket,
            buffer,
        }
    }

    pub fn recv<'a>(&'a self, buffer: &'a mut [u8]) -> RecvFuture<'a> {
        RecvFuture {
            socket: &self.socket,
            buffer,
        }
    }

    pub fn send_to<'a>(&'a self, buffer: &'a [u8], addr: SocketAddr) -> SendToFuture<'a> {
        SendToFuture {
            socket: &self.socket,
            buffer,
            addr,
        }
    }

    pub fn recv_from<'a>(&'a self, buffer: &'a mut [u8]) -> RecvFromFuture<'a> {
        RecvFromFuture {
            socket: &self.socket,
            buffer,
        }
    }

    pub fn send_batch<'a>(&'a self, packets: &'a [(&'a [u8], SocketAddr)]) -> SendBatchFuture<'a> {
        SendBatchFuture {
            socket: &self.socket,
            packets,
        }
    }

    pub fn recv_batch<'a>(&'a self, buffers: &'a mut [PacketBuf]) -> RecvBatchFuture<'a> {
        RecvBatchFuture {
            socket: &self.socket,
            buffers,
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

pub struct PacketBuf {
    buffer: Vec<u8>,
    pub len: usize,
    pub addr: Option<SocketAddr>,
}

impl PacketBuf {
    pub fn new(size: usize) -> Self {
        Self {
            buffer: vec![0; size],
            len: 0,
            addr: None,
        }
    }

    pub fn data(&self) -> &[u8] {
        &self.buffer[..self.len]
    }
}

macro_rules! io_future {
    ($name:ident, $output:ty, $call:expr, $interest:ident) => {
        impl Future for $name<'_> {
            type Output = io::Result<$output>;

            fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
                match $call(&mut self) {
                    Ok(output) => Poll::Ready(Ok(output)),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        REACTOR.$interest(self.socket.handle(), context.waker().clone());
                        Poll::Pending
                    }
                    Err(error) => Poll::Ready(Err(error)),
                }
            }
        }
    };
}

pub struct SendFuture<'a> {
    socket: &'a Socket,
    buffer: &'a [u8],
}
io_future!(
    SendFuture,
    usize,
    |this: &mut Pin<&mut SendFuture<'_>>| this.socket.send(this.buffer),
    register_write
);

pub struct RecvFuture<'a> {
    socket: &'a Socket,
    buffer: &'a mut [u8],
}
io_future!(
    RecvFuture,
    usize,
    |this: &mut Pin<&mut RecvFuture<'_>>| this.socket.recv(this.buffer),
    register_read
);

pub struct SendToFuture<'a> {
    socket: &'a Socket,
    buffer: &'a [u8],
    addr: SocketAddr,
}
io_future!(
    SendToFuture,
    usize,
    |this: &mut Pin<&mut SendToFuture<'_>>| this.socket.send_to(this.buffer, this.addr),
    register_write
);

pub struct RecvFromFuture<'a> {
    socket: &'a Socket,
    buffer: &'a mut [u8],
}
io_future!(
    RecvFromFuture,
    (usize, SocketAddr),
    |this: &mut Pin<&mut RecvFromFuture<'_>>| this.socket.recv_from(this.buffer),
    register_read
);

pub struct SendBatchFuture<'a> {
    socket: &'a Socket,
    packets: &'a [(&'a [u8], SocketAddr)],
}
io_future!(
    SendBatchFuture,
    usize,
    |this: &mut Pin<&mut SendBatchFuture<'_>>| this.socket.send_batch(this.packets),
    register_write
);

pub struct RecvBatchFuture<'a> {
    socket: &'a Socket,
    buffers: &'a mut [PacketBuf],
}

impl Future for RecvBatchFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        let socket = this.socket;
        let mut slices: Vec<&mut [u8]> = this
            .buffers
            .iter_mut()
            .map(|packet| packet.buffer.as_mut_slice())
            .collect();
        match socket.recv_batch(&mut slices) {
            Ok(received) => {
                for (packet, (len, addr)) in this.buffers.iter_mut().zip(received.iter()) {
                    packet.len = *len;
                    packet.addr = Some(*addr);
                }
                Poll::Ready(Ok(received.len()))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.register_read(socket.handle(), context.waker().clone());
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connected_echo() {
        let server = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        client.connect(server.local_addr().unwrap()).unwrap();
        server.connect(client.local_addr().unwrap()).unwrap();

        let mut buffer = [0; 4];
        let (received, sent) = tokio::join!(server.recv(&mut buffer), async {
            crate::sleep(std::time::Duration::from_millis(10)).await;
            client.send(b"ping").await
        });
        assert_eq!(sent.unwrap(), 4);
        assert_eq!(received.unwrap(), 4);
        assert_eq!(&buffer, b"ping");
    }

    #[tokio::test]
    async fn ipv6_echo() {
        let Ok(server) = UdpSocket::bind("[::1]:0".parse().unwrap()) else {
            return;
        };
        let client = UdpSocket::bind("[::1]:0".parse().unwrap()).unwrap();
        client
            .send_to(b"v6", server.local_addr().unwrap())
            .await
            .unwrap();
        let mut buffer = [0; 2];
        assert_eq!(server.recv_from(&mut buffer).await.unwrap().0, 2);
        assert_eq!(&buffer, b"v6");
    }

    #[tokio::test]
    async fn unconnected_and_batch_echo() {
        let server = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let server_addr = server.local_addr().unwrap();

        let payloads: Vec<Vec<u8>> = (0u8..32).map(|value| vec![value]).collect();
        let packets: Vec<_> = payloads
            .iter()
            .map(|payload| (payload.as_slice(), server_addr))
            .collect();
        assert_eq!(client.send_batch(&packets).await.unwrap(), 32);

        let mut buffers: Vec<_> = (0..32).map(|_| PacketBuf::new(1)).collect();
        assert_eq!(server.recv_batch(&mut buffers).await.unwrap(), 32);
        for (index, packet) in buffers.iter().enumerate() {
            assert_eq!(packet.data(), &[index as u8]);
            assert_eq!(packet.addr, Some(client.local_addr().unwrap()));
        }

        client.send_to(b"x", server_addr).await.unwrap();
        let mut buffer = [0; 1];
        let (len, from) = server.recv_from(&mut buffer).await.unwrap();
        assert_eq!(
            (len, from, buffer),
            (1, client.local_addr().unwrap(), *b"x")
        );
    }
}
