use futures::{StreamExt, pin_mut};
use llm::{CompletionChoice, Delta, StreamResponseChunk, ToolCallChunk, ToolCallFunction};
use serde_json::json;
use std::sync::Arc;
use stride_agent::{
    AgentConfig, BaseAgent, DEFAULT_MODEL, EventKind, InMemoryInteractionBroker, InteractionBroker,
    ModelRegEntry, ModelRegistry, NoopEventSink, TurnContext, tools::quiz::QuizTool,
};

fn registry(mock: &llm::Mock) -> ModelRegistry {
    let mut registry = ModelRegistry::new();
    registry.add_model(
        DEFAULT_MODEL,
        ModelRegEntry {
            api: mock.clone().into(),
            token: String::new(),
            model_name: "mock-model".to_string(),
            reasoning_effort: None,
            vision: false,
        },
    );
    registry
}

fn tool_call_chunk(name: &str, arguments: &str) -> StreamResponseChunk {
    StreamResponseChunk {
        id: "chunk".to_string(),
        object: "mock.stream".to_string(),
        created: 0,
        model: "mock-model".to_string(),
        system_fingerprint: None,
        usage: None,
        choices: vec![CompletionChoice {
            message: None,
            text: None,
            index: 0,
            delta: Some(Delta {
                content: None,
                thinking: None,
                tool_calls: Some(vec![ToolCallChunk {
                    index: Some(0),
                    id: Some("call_1".to_string()),
                    call_type: None,
                    function: Some(ToolCallFunction {
                        name: Some(name.to_string()),
                        arguments: Some(arguments.to_string()),
                    }),
                }]),
            }),
            logprobs: None,
            tool_calls: None,
            finish_reason: Some("tool_calls".to_string()),
        }],
    }
}

fn text_chunk(content: &str) -> StreamResponseChunk {
    StreamResponseChunk {
        id: "chunk".to_string(),
        object: "mock.stream".to_string(),
        created: 0,
        model: "mock-model".to_string(),
        system_fingerprint: None,
        usage: None,
        choices: vec![CompletionChoice {
            message: None,
            text: Some(content.to_string()),
            index: 0,
            delta: Some(Delta {
                content: Some(content.to_string()),
                thinking: None,
                tool_calls: None,
            }),
            logprobs: None,
            tool_calls: None,
            finish_reason: Some("stop".to_string()),
        }],
    }
}

#[test]
fn quiz_yields_questions_and_returns_answers() {
    futures::executor::block_on(async {
        let args = json!({
            "questions": [
                { "question": "Favorite color?", "options": ["red", "blue", "green"] },
                { "question": "Age?", "options": [] }
            ]
        });
        let mock = llm::Mock::new().with_stream_chunks(vec![
            vec![tool_call_chunk("quiz", &args.to_string())],
            vec![text_chunk("done")],
        ]);

        let agent = BaseAgent::new(
            DEFAULT_MODEL.to_string(),
            Arc::new(AgentConfig {
                model_registry: registry(&mock),
                max_iterations: 50,
                usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            String::new(),
            vec![],
        );
        agent.register_tool(QuizTool);

        let broker = Arc::new(InMemoryInteractionBroker::default());
        let stream = agent
            .make_turn(
                "ask".to_string(),
                vec![],
                TurnContext::new(
                    uuid::Uuid::now_v7(),
                    Arc::new(NoopEventSink),
                    broker.clone(),
                ),
            )
            .await;
        pin_mut!(stream);

        loop {
            let event = stream.next().await.unwrap();
            if let EventKind::QuizRequested { quiz_id, questions } = event.kind {
                assert_eq!(questions.len(), 2);
                assert_eq!(questions[0].question, "Favorite color?");
                assert_eq!(questions[0].options, vec!["red", "blue", "green"]);
                assert_eq!(questions[1].question, "Age?");
                assert!(questions[1].options.is_empty());
                assert!(broker.answer_quiz(quiz_id, vec!["blue".to_string(), "30".to_string()],));
                break;
            }
        }

        while stream.next().await.is_some() {}

        // Verify answers were passed back to LLM as tool result
        let requests = mock.stream_requests();
        assert_eq!(requests.len(), 2);
        let tool_result = requests[1]
            .messages
            .iter()
            .find(|m| m.role == llm::Role::Tool)
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&tool_result.content).unwrap();
        assert_eq!(result["answers"][0]["question"], "Favorite color?");
        assert_eq!(result["answers"][0]["answer"], "blue");
        assert_eq!(result["answers"][1]["question"], "Age?");
        assert_eq!(result["answers"][1]["answer"], "30");
    });
}

#[test]
fn quiz_questions_parses_args() {
    use stride_agent::Tool;

    let args = json!({
        "questions": [
            { "question": "Pick one", "options": ["a", "b"] }
        ]
    });
    let questions = QuizTool.quiz_questions(&args).unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].question, "Pick one");
    assert_eq!(questions[0].options, vec!["a", "b"]);
}

#[test]
fn quiz_questions_returns_none_on_invalid_args() {
    use stride_agent::Tool;

    let questions = QuizTool.quiz_questions(&json!({ "wrong": "field" }));
    assert!(questions.is_none());
}
