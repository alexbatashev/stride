# tinynet

Minimal, runtime-free async HTTP/1 client built on `mio` and `rustls`. No
`tokio`, no `hyper` client runtime — just a polling event loop and a TLS
handshake, so it can be embedded in code that must stay off a heavyweight
async runtime. Ships free functions for one-shot requests and a streaming
decoder for SSE/NDJSON bodies.

Part of [Stride](https://github.com/alexbatashev/stride).

## Usage

```rust
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use tinynet::send_request;

let req = Request::builder()
    .uri("https://example.com/mock")
    .body(Empty::<Bytes>::new())
    .unwrap();

let (status, body) = send_request(req).await.unwrap();
assert_eq!(status, 200);
```

Streaming responses (SSE/NDJSON) go through `stream_request`, which yields
chunks as they arrive instead of buffering the whole body.

## Configuration

- `TINYNET_TIMEOUT_SECS` — overall request deadline (connect + handshake +
  transfer). Defaults to 20s.
- `TINYNET_CONNECT_TIMEOUT_SECS` — per-address TCP connect timeout. Defaults
  to 10s.
