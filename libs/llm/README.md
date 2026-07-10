# llm

Unified async client for OpenAI-, Anthropic-, and Ollama-compatible chat
completion, embedding, and transcription APIs, plus a `Mock` backend for
tests. Built on [`tinynet`](../tinynet) rather than a full HTTP runtime, so
it stays light to embed.

Part of [Stride](https://github.com/alexbatashev/stride).

## Usage

```rust
use llm::{CompletionRequest, Message, OpenAI, Role};

let client = OpenAI::new("https://api.openai.com/v1");

let request = CompletionRequest::new(
    "gpt-4o-mini",
    &[Message {
        role: Role::User,
        content: "Hello!".to_string(),
        images: None,
        thinking: None,
        tool_calls: None,
        tool_call_id: None,
    }],
)
.max_tokens(32)
.temperature(0.0);

let completion = client.get_completion("sk-...", request).await?;
```

`Anthropic` and `Ollama` expose the same shape of API; `API` is an enum
wrapper over all backends (including `Mock`) for call sites that need to pick
a provider at runtime.
