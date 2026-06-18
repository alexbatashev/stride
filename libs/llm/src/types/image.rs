use serde::{Deserialize, Serialize};

/// A reference to an image attached to a [`crate::Message`]. The image is
/// described either by a publicly reachable `url` or by inline base64 `data`
/// (with its `mime_type`). Providers that accept a single URL use
/// [`ImageSource::as_request_url`], which prefers the public URL and otherwise
/// builds a `data:` URI from the inline bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImageSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl ImageSource {
    /// Image reachable at a public URL.
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            data: None,
            mime_type: None,
        }
    }

    /// Image carried inline as base64-encoded bytes (without a `data:` prefix).
    pub fn base64(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            url: None,
            data: Some(data.into()),
            mime_type: Some(mime_type.into()),
        }
    }

    /// A single URL usable directly by an image-capable API: the public URL when
    /// present, otherwise a `data:` URI built from the inline base64 bytes.
    pub fn as_request_url(&self) -> Option<String> {
        if let Some(url) = &self.url {
            return Some(url.clone());
        }
        let data = self.data.as_ref()?;
        let mime = self.mime_type.as_deref().unwrap_or("image/png");
        Some(format!("data:{mime};base64,{data}"))
    }
}
