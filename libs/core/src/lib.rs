pub mod chat;
pub mod data;
pub mod js;
pub mod tools;

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::{SinkExt, Stream};
use std::io::Read;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use uuid::Uuid;

uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Uuid::parse_str(&value).map_err(uniffi::deps::anyhow::Error::msg),
});

uniffi::setup_scaffolding!();

#[derive(Clone)]
struct CoreHttpTransport;

impl llm::HttpTransport for CoreHttpTransport {
    fn request<'a>(
        &'a self,
        request: llm::HttpRequest,
    ) -> BoxFuture<'a, Result<llm::HttpResponse, llm::Error>> {
        let (tx, rx) = oneshot::channel();

        gcd_global_queue().exec_async(move || {
            let _ = tx.send(request_blocking(request));
        });

        Box::pin(async move {
            rx.await
                .map_err(|_| llm::Error::RequestError("transport task dropped".to_string()))?
        })
    }

    fn request_stream<'a>(
        &'a self,
        request: llm::HttpRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<Vec<u8>, llm::Error>> + Send + 'a>> {
        let (tx, rx) = mpsc::unbounded();

        gcd_global_queue().exec_async(move || {
            stream_request_blocking(request, tx);
        });

        Box::pin(rx)
    }
}

fn gcd_global_queue() -> dispatch2::DispatchRetained<dispatch2::DispatchQueue> {
    dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::Priority(
        dispatch2::DispatchQueueGlobalPriority::Default,
    ))
}

fn request_blocking(request: llm::HttpRequest) -> Result<llm::HttpResponse, llm::Error> {
    let llm::HttpRequest {
        method,
        url,
        headers,
        body,
    } = request;

    let mut req = ureq::request(&method, &url);
    for (k, v) in headers {
        req = req.set(&k, &v);
    }

    let response = if body.is_empty() {
        req.call()
    } else {
        req.send_bytes(&body)
    };

    let (status, mut reader): (u16, Box<dyn Read>) = match response {
        Ok(resp) => (resp.status(), Box::new(resp.into_reader())),
        Err(ureq::Error::Status(status, resp)) => (status, Box::new(resp.into_reader())),
        Err(ureq::Error::Transport(err)) => {
            return Err(llm::Error::RequestError(err.to_string()));
        }
    };

    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| llm::Error::ParsingError(e.to_string()))?;

    Ok(llm::HttpResponse {
        status,
        body: bytes,
    })
}

fn stream_request_blocking(
    request: llm::HttpRequest,
    mut tx: mpsc::UnboundedSender<Result<Vec<u8>, llm::Error>>,
) {
    let llm::HttpRequest {
        method,
        url,
        headers,
        body,
    } = request;

    let mut req = ureq::request(&method, &url);
    for (k, v) in headers {
        req = req.set(&k, &v);
    }

    let response = if body.is_empty() {
        req.call()
    } else {
        req.send_bytes(&body)
    };

    let mut reader = match response {
        Ok(resp) => {
            if !(200..300).contains(&resp.status()) {
                let _ = tx.start_send(Err(llm::Error::ServerError(resp.status())));
                return;
            }
            resp.into_reader()
        }
        Err(ureq::Error::Status(status, _)) => {
            let _ = tx.start_send(Err(llm::Error::ServerError(status)));
            return;
        }
        Err(ureq::Error::Transport(err)) => {
            let _ = tx.start_send(Err(llm::Error::RequestError(err.to_string())));
            return;
        }
    };

    let mut buf = [0_u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                if tx.start_send(Ok(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = tx.start_send(Err(llm::Error::RequestError(e.to_string())));
                return;
            }
        }
    }
}

pub fn get_llm_transport() -> llm::TransportHandle {
    static LLM_TRANSPORT: OnceLock<Arc<dyn llm::HttpTransport>> = OnceLock::new();
    let transport = LLM_TRANSPORT.get_or_init(|| Arc::new(CoreHttpTransport));
    llm::TransportHandle::new(transport.clone())
}
