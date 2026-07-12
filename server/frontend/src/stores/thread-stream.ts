import {store} from '@frontiers-labs/argon';

export type LiveThreadMessage = {
	id: string;
	seq: number;
	content: string;
	thinking: string;
	agentPath: string[];
	committed: boolean;
};

export type LiveToolCall = {
	id: string;
	seq: number;
	createdAt: number;
	name: string;
	arguments: string;
	result: string;
	isError: boolean;
	status: 'running' | 'finished';
	agentPath: string[];
};

export type Subagent = {
	id: string;
	name: string;
	model: string;
	result: string;
	finished: boolean;
	parentToolCallId: string;
	/// Full slash-joined UUID path of this agent (matches `thread_agents.agent_path`).
	agentPath: string;
	/// ms-since-epoch used only for stable list ordering.
	createdAt: number;
};

/// Live + REST-hydrated transcript for one subagent. `key` is the agent's full
/// slash-joined path (matches `thread_agents.agent_path`). Stored as an array
/// rather than a map so the SSR store can encode it as a typed struct.
export type AgentTranscript = {
	key: string;
	messages: LiveThreadMessage[];
	toolCalls: LiveToolCall[];
};

export const threadStream = store({
	threadId: '',
	running: false,
	messages: [] as LiveThreadMessage[],
	toolCalls: [] as LiveToolCall[],
	subagents: [] as Subagent[],
	agentTranscripts: [] as AgentTranscript[],
	pendingApprovals: [] as {id: string; toolCallId: string; message: string}[],
	pendingQuizzes: [] as {id: string; questions: {question: string; options: string[]}[]}[],
});
