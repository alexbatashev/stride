use crate::{
    API, Completion, CompletionChoice, CompletionRequest, Delta, Error, Message, ModelDesc, Role,
    StreamResponseChunk, Usage,
};
use futures::{Stream, stream};
use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

type RequestMatcher = Arc<dyn Fn(&CompletionRequest) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct Mock {
    stream_chunks: Arc<Mutex<VecDeque<Vec<StreamResponseChunk>>>>,
    rules: Arc<Mutex<Vec<MockRule>>>,
    default_chunks: Arc<Mutex<Option<Vec<StreamResponseChunk>>>>,
    stream_requests: Arc<Mutex<Vec<CompletionRequest>>>,
}

#[derive(Clone)]
struct MockRule {
    matcher: RequestMatcher,
    chunks: Vec<StreamResponseChunk>,
}

pub struct MockRuleBuilder {
    mock: Mock,
    matcher: RequestMatcher,
}

impl MockRuleBuilder {
    pub fn respond(self, chunks: Vec<StreamResponseChunk>) -> Mock {
        self.mock.rules.lock().unwrap().push(MockRule {
            matcher: self.matcher,
            chunks,
        });
        self.mock
    }
}

impl fmt::Debug for Mock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Mock")
            .field(
                "scripted_response_count",
                &self.stream_chunks.lock().unwrap().len(),
            )
            .field("rule_count", &self.rules.lock().unwrap().len())
            .field(
                "has_default_response",
                &self.default_chunks.lock().unwrap().is_some(),
            )
            .field("request_count", &self.stream_requests.lock().unwrap().len())
            .finish()
    }
}

impl Mock {
    pub fn new() -> Self {
        Mock {
            stream_chunks: Arc::new(Mutex::new(VecDeque::new())),
            rules: Arc::new(Mutex::new(Vec::new())),
            default_chunks: Arc::new(Mutex::new(None)),
            stream_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_stream_chunks(self, chunks: Vec<Vec<StreamResponseChunk>>) -> Self {
        *self.stream_chunks.lock().unwrap() = chunks.into();
        self
    }

    pub fn when(
        self,
        matcher: impl Fn(&CompletionRequest) -> bool + Send + Sync + 'static,
    ) -> MockRuleBuilder {
        MockRuleBuilder {
            mock: self,
            matcher: Arc::new(matcher),
        }
    }

    pub fn default_response(self, chunks: Vec<StreamResponseChunk>) -> Self {
        *self.default_chunks.lock().unwrap() = Some(chunks);
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
        let matching_chunks = self
            .rules
            .lock()
            .unwrap()
            .iter()
            .find(|rule| (rule.matcher)(&request))
            .map(|rule| rule.chunks.clone());
        self.stream_requests.lock().unwrap().push(request);
        if let Some(chunks) = matching_chunks {
            return Box::pin(stream::iter(chunks.into_iter().map(Ok)));
        }

        if let Some(chunks) = self.stream_chunks.lock().unwrap().pop_front() {
            return Box::pin(stream::iter(chunks.into_iter().map(Ok)));
        }

        if let Some(chunks) = self.default_chunks.lock().unwrap().clone() {
            return Box::pin(stream::iter(chunks.into_iter().map(Ok)));
        }

        let chunk = StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            usage: None,
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

#[cfg(test)]
mod tests {
    use futures::{StreamExt, executor::block_on};

    use super::*;

    fn chunk(text: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            choices: vec![CompletionChoice {
                delta: Some(Delta {
                    content: Some(text.to_owned()),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn request(content: &str, role: Role) -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message {
                role,
                content: content.to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn matcher_selects_response_from_request() {
        let mock = Mock::new()
            .when(|request| request.last_tool_result_contains("weather"))
            .respond(vec![chunk("sunny")])
            .default_response(vec![chunk("fallback")]);

        let response = block_on(
            mock.stream_completion("", request("weather: 20C", Role::Tool))
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            response[0].as_ref().unwrap().choices[0]
                .delta
                .as_ref()
                .unwrap()
                .content
                .as_deref(),
            Some("sunny")
        );
    }

    #[test]
    fn default_response_handles_unmatched_request() {
        let mock = Mock::new()
            .when(|request| request.last_message_contains("expected"))
            .respond(vec![chunk("matched")])
            .default_response(vec![chunk("fallback")]);

        let response = block_on(
            mock.stream_completion("", request("different", Role::User))
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            response[0].as_ref().unwrap().choices[0]
                .delta
                .as_ref()
                .unwrap()
                .content
                .as_deref(),
            Some("fallback")
        );
    }
}
