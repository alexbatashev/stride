import { ThreadMessage, listAgentMessages, listAgents } from "../api/threads.js";
import { AgentTranscript, Subagent, threadStream } from "../stores/thread-stream.js";
import { findTranscript, upsertTranscript } from "./agent-transcripts.js";

// Loads the persisted subagent registry into `threadStream.subagents`, merging
// with any agents already seen live (live entries win — they carry the freshest
// status during an active run; on a cold reload there are none, so REST fills).
export async function loadSubagents(threadId: string): Promise<void> {
	const agents = await listAgents(threadId);
	const byId = new Map<string, Subagent>();
	for (const agent of agents) {
		byId.set(agent.agent_id, {
			id: agent.agent_id,
			name: agent.name,
			model: agent.model,
			result: agent.result ?? "",
			finished: agent.finished,
			parentToolCallId: agent.parent_tool_call_id ?? "",
			agentPath: agent.agent_path,
			createdAt: agent.created_at,
		});
	}
	for (const live of threadStream.subagents) byId.set(live.id, live);
	threadStream.subagents = [...byId.values()];
}

// Loads one subagent's transcript (its own messages plus descendants') into its
// bucket, merging with any live events already received. REST rows win for
// committed content; live-only rows (not yet persisted) are preserved.
export async function loadSubagentTranscript(
	threadId: string,
	agentId: string,
	agentPath: string,
): Promise<void> {
	const messages = await listAgentMessages(threadId, agentId);
	const existing = findTranscript(agentPath);
	const bucket: AgentTranscript = {
		key: agentPath,
		messages: existing ? [...existing.messages] : [],
		toolCalls: existing ? [...existing.toolCalls] : [],
	};
	mergeRestMessages(bucket, messages);
	threadStream.agentTranscripts = upsertTranscript(bucket);
}

// Folds the flat REST transcript (assistant messages carrying `tool_calls`
// arrays plus separate tool-output rows) into the bucket's live-shaped
// messages/toolCalls, upserting by id so committed content replaces any partial.
function mergeRestMessages(bucket: AgentTranscript, messages: ThreadMessage[]): void {
	const outputs = new Map(
		messages.filter((message) => message.tool_call_id).map((message) => [message.tool_call_id, message]),
	);
	for (const message of messages) {
		if (message.role !== "agent") continue;
		upsertMessage(bucket, {
			id: message.id,
			seq: message.seq,
			content: message.content,
			thinking: message.thinking ?? "",
			agentPath: [],
			committed: true,
		});
		for (const call of message.tool_calls) {
			const output = outputs.get(call.id);
			upsertToolCall(bucket, {
				id: call.id,
				seq: message.seq,
				createdAt: message.created_at ?? message.seq,
				name: call.name,
				arguments: call.arguments,
				result: output?.content ?? "",
				isError: false,
				status: output ? "finished" : "running",
				agentPath: [],
			});
		}
	}
}

function upsertMessage(bucket: AgentTranscript, message: AgentTranscript["messages"][number]): void {
	bucket.messages = [...bucket.messages.filter((existing) => existing.id !== message.id), message];
}

function upsertToolCall(bucket: AgentTranscript, tool: AgentTranscript["toolCalls"][number]): void {
	bucket.toolCalls = [...bucket.toolCalls.filter((existing) => existing.id !== tool.id), tool];
}
