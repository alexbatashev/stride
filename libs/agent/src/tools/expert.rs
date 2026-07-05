use crate::ToolRegistry;

use super::subagent::FixedModelSubAgentTool;

pub const EXPERT_MODEL: &str = "expert";
pub const EXPERT_NAME: &str = "expert";

const SYSTEM_PROMPT: &str = "You are an expert reasoning agent. Help the main agent with hard analysis, design tradeoffs, debugging, research synthesis, and other tasks where deeper reasoning materially improves the answer.
Use available tools when they are needed to verify facts or inspect source material. Do not do broad exploration without a clear reason. State important assumptions, uncertainty, and risks.
Return a self-contained answer the main agent can act on. Prefer concise structure. Include concrete recommendations, evidence, and next steps when relevant. Do not ask the user questions directly; give the main agent the clearest possible guidance.";

pub fn make_expert(tool_registry: ToolRegistry) -> FixedModelSubAgentTool {
    FixedModelSubAgentTool::new(
        EXPERT_NAME,
        "Expert",
        "A stronger expert subagent for difficult reasoning tasks. Use this tool to consult a more capable model when deeper analysis materially improves the outcome: architecture decisions, subtle debugging, security or correctness analysis, research synthesis, comparing tradeoffs, investigating ambiguous failures, reviewing a risky plan, or answering a tricky question where an independent second pass is valuable. Do not use it for routine implementation, simple lookups, mechanical edits, summarizing obvious context, or questions you can answer confidently yourself. Before calling the expert, gather the immediate context yourself and decide what specific judgment you need. In the prompt, include the task goal, relevant facts, constraints, code snippets or source summaries, what you have already checked, open questions, and the desired shape of the answer. Treat the expert response as advice: verify assumptions, integrate useful conclusions, and make the final decision yourself.",
        EXPERT_MODEL,
        SYSTEM_PROMPT,
        tool_registry,
    )
}
