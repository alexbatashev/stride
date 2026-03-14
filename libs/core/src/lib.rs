pub mod api;
pub mod chat;
pub mod data;
pub mod grpc;
pub mod js;
pub mod tools;

use uuid::Uuid;

uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Uuid::parse_str(&value).map_err(uniffi::deps::anyhow::Error::msg),
});

uniffi::setup_scaffolding!();
