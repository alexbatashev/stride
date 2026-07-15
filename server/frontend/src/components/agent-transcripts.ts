import { AgentTranscript, threadStream } from "../stores/thread-stream.js";

// Small helpers over `threadStream.agentTranscripts` (an array keyed by the
// agent's full path). Kept out of the store module because store files only
// export the store itself.

export function findTranscript(key: string): AgentTranscript | undefined {
	return threadStream.agentTranscripts.find((bucket) => bucket.key === key);
}

export function upsertTranscript(bucket: AgentTranscript): AgentTranscript[] {
	return [...threadStream.agentTranscripts.filter((existing) => existing.key !== bucket.key), bucket];
}
