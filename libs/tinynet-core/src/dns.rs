use std::{
    future::Future,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

struct State {
    result: Option<io::Result<Vec<SocketAddr>>>,
    waker: Option<Waker>,
}

pub struct ResolveFuture {
    state: Arc<Mutex<State>>,
}

pub fn resolve(host: &str, port: u16) -> ResolveFuture {
    let state = Arc::new(Mutex::new(State {
        result: None,
        waker: None,
    }));

    if let Ok(ip) = host.parse::<IpAddr>() {
        state.lock().unwrap().result = Some(Ok(vec![SocketAddr::new(ip, port)]));
        return ResolveFuture { state };
    }

    let worker_state = Arc::clone(&state);
    let host = host.to_owned();
    std::thread::Builder::new()
        .name("tinynet-dns".into())
        .spawn(move || {
            let result = (host.as_str(), port).to_socket_addrs().map(|addresses| {
                let mut addresses: Vec<_> = addresses.collect();
                addresses.sort_by_key(|address| address.is_ipv6());
                addresses.dedup();
                addresses
            });
            let waker = {
                let mut state = worker_state.lock().unwrap();
                state.result = Some(result);
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        })
        .expect("tinynet: failed to spawn DNS resolver thread");

    ResolveFuture { state }
}

impl Future for ResolveFuture {
    type Output = io::Result<Vec<SocketAddr>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.result.take() {
            Poll::Ready(result)
        } else {
            state.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn literals_resolve_directly() {
        assert_eq!(
            resolve("127.0.0.1", 80).await.unwrap(),
            ["127.0.0.1:80".parse().unwrap()]
        );
        assert_eq!(
            resolve("::1", 443).await.unwrap(),
            ["[::1]:443".parse().unwrap()]
        );
    }

    #[tokio::test]
    async fn localhost_is_ipv4_first() {
        let addresses = resolve("localhost", 80).await.unwrap();
        if let Some(first_ipv6) = addresses.iter().position(SocketAddr::is_ipv6) {
            assert!(addresses[..first_ipv6].iter().all(SocketAddr::is_ipv4));
        }
    }

    #[tokio::test]
    async fn invalid_name_returns_error() {
        assert!(resolve("tinynet-does-not-exist.invalid", 80).await.is_err());
    }
}
