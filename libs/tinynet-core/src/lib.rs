pub mod dns;
#[cfg(feature = "http1")]
mod protocol;
mod reactor;
mod sys;
#[cfg(feature = "rustls")]
mod tls;

pub mod buf;
#[cfg(feature = "client")]
pub mod client;
pub mod codec;
pub mod http;
pub mod tcp;
pub mod udp;
pub mod utils;

#[cfg(feature = "http1")]
pub use protocol::Protocol;
pub use reactor::{Sleep, deadline, sleep};
