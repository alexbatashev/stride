use crate::{
    API, Completion, CompletionChoice, CompletionRequest, Delta, Error, Message, ModelDesc, Role,
    StreamResponseChunk, Usage,
};
use futures::{Stream, stream};
use std::pin::Pin;

#[derive(Debug, Clone)]
pub struct Mock;

impl Mock {
    pub fn new() -> Self {
        Mock
    }

    pub async fn list_models(&self, _token: &str) -> Result<Vec<ModelDesc>, Error> {
        Ok(vec![ModelDesc {
            id: "mock-model".to_string(),
            object: "model".to_string(),
            created: Some(0),
            owned_by: Some("mock-owner".to_string()),
        }])
    }

    pub async fn get_model(&self, _token: &str, model: &str) -> Result<ModelDesc, Error> {
        Ok(ModelDesc {
            id: model.to_string(),
            object: "model".to_string(),
            created: Some(0),
            owned_by: Some("mock-owner".to_string()),
        })
    }

    pub async fn get_completion(
        &self,
        _token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let message = Some(Message {
            role: Role::Assistant,
            content: format!("Echo: {:?}", request.messages),
        });
        Ok(Completion {
            id: "mock-completion-id".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            choices: vec![CompletionChoice {
                message,
                text: Some("This is a mock completion.".to_string()),
                index: 0,
                delta: None,
                logprobs: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        })
    }

    pub fn stream_completion(
        &self,
        _token: &str,
        _request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let chunk = StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: Some("Partial mock stream response.".to_string()),
                index: 0,
                delta: Some(Delta {
                    content: Some("Partial mock stream response.".to_string()),
                }),
                logprobs: None,
                finish_reason: Some("stop".to_string()),
            }],
        };
        Box::pin(stream::once(async move { Ok(chunk) }))
    }
}

impl Into<API> for Mock {
    fn into(self) -> API {
        API::Mock(self)
    }
}
