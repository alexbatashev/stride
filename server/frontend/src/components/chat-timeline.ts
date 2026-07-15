import type { ThreadMessage } from "../api/threads.js";
import type { TimelineMessage } from "../shared/timeline.js";
import { threadStream } from "../stores/thread-stream.js";

type ViewMessage = ThreadMessage & { pending?: boolean; liveToolName?: string; liveToolDetail?: string; liveToolError?: boolean };

function messageTimestamp(message: ViewMessage): number {
  if ((message.created_at ?? 0) > 0) return message.created_at ?? 0;
  const pendingTimestamp = message.id.match(/pending-user-(\d+)/)?.[1];
  if (pendingTimestamp) return Number(pendingTimestamp);
  const hex = message.id.replaceAll("-", "").slice(0, 12);
  const timestamp = Number.parseInt(hex, 16);
  return Number.isFinite(timestamp) ? timestamp : Date.now();
}

export function summarizeToolArguments(argumentsText: string): string {
  if (!argumentsText) return "";
  try {
    const value = JSON.parse(argumentsText) as Record<string, unknown>;
    for (const key of ["path", "command", "query", "url"]) {
      if (typeof value[key] === "string" && value[key] !== "") return value[key];
    }
  } catch {
    return "";
  }
  return "";
}

function isSubagentTool(name: string): boolean {
  const normalized = name.toLowerCase().replaceAll("-", "_");
  return normalized.includes("subagent") || normalized.includes("spawn_agent");
}

function toolActivityLabel(name: string): string {
  const normalized = name.toLowerCase().replaceAll("-", "_");
  if (normalized.includes("command") || normalized.includes("shell") || normalized.includes("exec")) return "Ran command";
  if (normalized.includes("read_file") || normalized.endsWith("read")) return "Read file";
  if (normalized.includes("apply_patch") || normalized.includes("write") || normalized.includes("edit")) return "Changed files";
  if (normalized.includes("search") || normalized.includes("find") || normalized.endsWith("rg")) return "Searched files";
  return name.replaceAll("_", " ");
}

function plainMessage(message: ViewMessage): TimelineMessage {
  return {
    id: message.id,
    seq: message.seq,
    createdAt: messageTimestamp(message),
    role: message.role,
    messageType: message.liveToolName ? "tool_activity" : "",
    format: message.format,
    content: message.content || (message.pending ? "Thinking…" : ""),
    thinking: message.thinking ?? "",
    toolName: message.liveToolName ? toolActivityLabel(message.liveToolName) : message.tool_call_name ?? (message.role === "tool" ? "Tool output" : ""),
    toolDetail: summarizeToolArguments(message.liveToolDetail ?? ""),
    pending: message.pending ?? false,
    status: message.pending ? "running" : "finished",
    isError: message.liveToolError ?? false,
  };
}

export function buildClientTimeline(messages: ViewMessage[]): TimelineMessage[] {
  const timeline: TimelineMessage[] = [];
  const consumed = new Set<string>();
  for (const message of messages) {
    if (message.tool_calls.length === 0) continue;
    if (message.content !== "" || message.thinking) timeline.push(plainMessage(message));
    for (const call of message.tool_calls) {
      const output = messages.find((candidate) => candidate.tool_call_id === call.id);
      if (output) consumed.add(output.id);
      const subagent = isSubagentTool(call.name)
        ? threadStream.subagents.find((candidate) => candidate.parentToolCallId === call.id)
        : undefined;
      timeline.push({
        id: `tool:${call.id}`,
        seq: message.seq,
        createdAt: messageTimestamp(message),
        role: "tool",
        messageType: "tool_activity",
        format: output?.format ?? "markdown",
        content: output?.content ?? "",
        thinking: "",
        toolName: subagent?.name ?? toolActivityLabel(call.name),
        toolDetail: summarizeToolArguments(call.arguments),
        pending: !output,
        status: output ? "finished" : "running",
        isError: false,
        subagentKey: subagent?.agentPath,
      });
    }
  }
  for (const message of messages) {
    if (message.tool_calls.length > 0 || consumed.has(message.id)) continue;
    if (message.liveToolName && isSubagentTool(message.liveToolName)) {
      const subagent = threadStream.subagents.find((candidate) => candidate.parentToolCallId === message.tool_call_id);
      if (subagent) {
        const item = plainMessage(message);
        item.toolName = subagent.name;
        item.subagentKey = subagent.agentPath;
        timeline.push(item);
      }
      continue;
    }
    timeline.push(plainMessage(message));
  }
  return timeline.sort((left, right) => left.seq - right.seq);
}

// Builds a subagent's chat timeline from `threadStream.agentTranscripts`,
// aggregating the selected agent's own bucket plus its descendants' buckets
// (path prefix). Unlike the root timeline, nested subagent spawns are kept as
// tool cards. Deduped by id so live and REST rows for the same message merge.
export function buildSubagentTimeline(agentKey: string): TimelineMessage[] {
  const timeline: TimelineMessage[] = [];
  const seen = new Set<string>();
  const push = (item: TimelineMessage) => {
    if (seen.has(item.id)) return;
    seen.add(item.id);
    timeline.push(item);
  };
  for (const bucket of threadStream.agentTranscripts) {
    if (bucket.key !== agentKey && !bucket.key.startsWith(`${agentKey}/`)) continue;
    for (const message of bucket.messages) {
      if (!message.content && !message.thinking) continue;
      push({
        id: message.id,
        seq: message.seq,
        createdAt: message.seq,
        role: "agent",
        messageType: "",
        format: "markdown",
        content: message.content,
        thinking: message.thinking ?? "",
        toolName: "",
        toolDetail: "",
        pending: !message.committed,
        status: message.committed ? "finished" : "running",
        isError: false,
      });
    }
    for (const tool of bucket.toolCalls) {
      push({
        id: `tool:${tool.id}`,
        seq: tool.seq,
        createdAt: tool.createdAt,
        role: "tool",
        messageType: "tool_activity",
        format: "markdown",
        content: tool.status === "running" && !tool.result ? "" : tool.result,
        thinking: "",
        toolName: toolActivityLabel(tool.name),
        toolDetail: summarizeToolArguments(tool.arguments),
        pending: tool.status === "running",
        status: tool.status,
        isError: tool.isError,
      });
    }
  }
  return timeline.sort((left, right) => left.seq - right.seq);
}
