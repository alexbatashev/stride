pub mod chat;
pub mod data;
pub mod js;
pub mod tools;

use std::sync::OnceLock;
use std::{future::Future, pin::Pin};
use uuid::Uuid;

uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Uuid::parse_str(&value).map_err(uniffi::deps::anyhow::Error::msg),
});

uniffi::setup_scaffolding!();

static TOKIO_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub(crate) fn tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime")
    })
}

pub fn get_llm_runtime() -> llm::SharedExecutor {
    #[derive(Clone)]
    struct CoreExecutor;

    impl hyper::rt::Executor<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> for CoreExecutor {
        fn execute(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            tokio_runtime().spawn(fut);
        }
    }

    llm::SharedExecutor::new(CoreExecutor)
}
