import {store} from '@frontiers-labs/argon';

export type LiveThreadMessage = {
	id: string;
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

export const threadStream = store({
	threadId: '',
	running: false,
	messages: [] as LiveThreadMessage[],
	toolCalls: [] as LiveToolCall[],
	subagents: [] as {id: string; name: string; model: string; result: string; finished: boolean; parentToolCallId: string}[],
	pendingApprovals: [] as {id: string; toolCallId: string; message: string}[],
	pendingQuizzes: [] as {id: string; questions: {question: string; options: string[]}[]}[],
});
