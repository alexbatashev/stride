use crate::{
    API, Completion, CompletionChoice, CompletionRequest, Delta, Error, Message, ModelDesc, Role,
    StreamResponseChunk, Usage,
};
use futures::{Stream, stream};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Mock {
    stream_chunks: Arc<Mutex<VecDeque<Vec<StreamResponseChunk>>>>,
    stream_requests: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl Mock {
    pub fn new() -> Self {
        Mock {
            stream_chunks: Arc::new(Mutex::new(VecDeque::new())),
            stream_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_stream_chunks(self, chunks: Vec<Vec<StreamResponseChunk>>) -> Self {
        *self.stream_chunks.lock().unwrap() = chunks.into();
        self
    }

    pub fn stream_requests(&self) -> Vec<CompletionRequest> {
        self.stream_requests.lock().unwrap().clone()
    }

    pub async fn list_models(&self, _token: &str) -> Result<Vec<ModelDesc>, Error> {
        Ok(vec![ModelDesc {
            id: "mock-model".to_string(),
            ..Default::default()
        }])
    }

    pub async fn get_model(&self, _token: &str, model: &str) -> Result<ModelDesc, Error> {
        Ok(ModelDesc {
            id: model.to_string(),
            ..Default::default()
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
            images: None,
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
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
                tool_calls: None,
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
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        self.stream_requests.lock().unwrap().push(request);

        if let Some(chunks) = self.stream_chunks.lock().unwrap().pop_front() {
            return Box::pin(stream::iter(chunks.into_iter().map(Ok)));
        }

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
                    thinking: None,
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        };
        Box::pin(stream::once(async move { Ok(chunk) }))
    }
}

impl Default for Mock {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Mock> for API {
    fn from(val: Mock) -> API {
        API::Mock(val)
    }
}
