use std::pin::Pin;

use futures::Stream;

use crate::{
    Anthropic, Completion, CompletionRequest, EmbeddingResponse, Error, Mock, ModelDesc, Ollama,
    OpenAI, StreamResponseChunk, Transcription,
};

#[derive(Debug, Clone)]
pub enum API {
    OpenAI(OpenAI),
    Anthropic(Anthropic),
    Ollama(Ollama),
    Mock(Mock),
}

impl API {
    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        match self {
            API::OpenAI(api) => api.list_models(token).await,
            API::Anthropic(api) => api.list_models(token).await,
            API::Ollama(api) => api.list_models(token).await,
            API::Mock(api) => api.list_models(token).await,
        }
    }

    pub async fn get_model(&self, token: &str, model_name: &str) -> Result<ModelDesc, Error> {
        match self {
            API::OpenAI(api) => api.get_model(token, model_name).await,
            API::Anthropic(api) => api.get_model(token, model_name).await,
            API::Ollama(api) => api.get_model(token, model_name).await,
            API::Mock(api) => api.get_model(token, model_name).await,
        }
    }

    pub async fn get_embeddings(
        &self,
        token: &str,
        input: &str,
        model: &str,
    ) -> Result<EmbeddingResponse, Error> {
        match self {
            API::OpenAI(api) => api.get_embeddings(token, input, model).await,
            API::Ollama(api) => api.get_embeddings(token, input, model).await,
            _ => Err(Error::InvalidRequest(
                "embeddings are only supported by OpenAI- and Ollama-compatible providers"
                    .to_owned(),
            )),
        }
    }

    pub async fn transcribe(
        &self,
        token: &str,
        audio: &[u8],
        file_name: &str,
        mime_type: &str,
        model: &str,
    ) -> Result<Transcription, Error> {
        match self {
            API::OpenAI(api) => {
                api.transcribe(token, audio, file_name, mime_type, model)
                    .await
            }
            _ => Err(Error::InvalidRequest(
                "audio transcription is only supported by OpenAI-compatible providers".to_owned(),
            )),
        }
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        if request.stream.unwrap_or_default() {
            return Err(Error::InvalidRequest("expected stream == false".to_owned()));
        }
        match self {
            API::OpenAI(api) => api.get_completion(token, request).await,
            API::Anthropic(api) => api.get_completion(token, request).await,
            API::Ollama(api) => api.get_completion(token, request).await,
            API::Mock(api) => api.get_completion(token, request).await,
        }
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let request = request.stream();
        match self {
            API::OpenAI(api) => api.stream_completion(token, request),
            API::Anthropic(api) => api.stream_completion(token, request),
            API::Ollama(api) => api.stream_completion(token, request),
            API::Mock(api) => api.stream_completion(token, request),
        }
    }
}

impl From<Anthropic> for API {
    fn from(val: Anthropic) -> API {
        API::Anthropic(val)
    }
}

impl From<Ollama> for API {
    fn from(val: Ollama) -> API {
        API::Ollama(val)
    }
}

impl From<OpenAI> for API {
    fn from(val: OpenAI) -> API {
        API::OpenAI(val)
    }
}
