export interface TimelineMessage {
  id: string;
  seq: number;
  createdAt: number;
  role: string;
  messageType: string;
  format: string;
  content: string;
  thinking: string;
  toolName: string;
  toolDetail: string;
  pending: boolean;
  status: string;
  isError: boolean;
}

export interface TimelineItem {
  id: string;
  seq: number;
  createdAt: number;
  role: string;
  kind: string;
  format: string;
  text: string;
  thinking: string;
  toolName: string;
  toolDetail: string;
  status: string;
  isError: boolean;
  pending: boolean;
}

export interface WorkSegment {
  id: string;
  commentary: string;
  tools: TimelineItem[];
}

export interface ChatTurn {
  id: string;
  hasUser: boolean;
  user: TimelineItem;
  hasWork: boolean;
  segments: WorkSegment[];
  hasAnswer: boolean;
  answer: TimelineItem;
  running: boolean;
  startedAt: number;
  workLabel: string;
}

export function decodeLegacyText(value: string): string {
  return value
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&#39;", "'")
    .replaceAll("&apos;", "'")
    .replaceAll("&amp;", "&");
}

export function timelineKind(message: TimelineMessage): string {
  if (message.messageType !== "") return message.messageType;
  if (message.role === "tool") return "tool_output";
  if (message.role === "system") return "agent";
  return message.role;
}

export function isSubagentTool(name: string): boolean {
  const normalized = name.toLowerCase().replaceAll("-", "_");
  return normalized.includes("subagent") || normalized.includes("spawn_agent");
}

export function buildTimeline(messages: TimelineMessage[]): TimelineItem[] {
  return messages.map((message) => ({
    id: message.id,
    seq: message.seq,
    createdAt: message.createdAt,
    role: message.role,
    kind: timelineKind(message),
    format: message.format,
    text: message.format === "markdown" ? decodeLegacyText(message.content) : message.content,
    thinking: decodeLegacyText(message.thinking),
    toolName: message.toolName,
    toolDetail: message.toolDetail,
    status: message.status,
    isError: message.isError,
    pending: message.pending,
  }));
}

function emptyTimelineItem(id: string): TimelineItem {
  return {
    id,
    seq: 0,
    createdAt: 0,
    role: "agent",
    kind: "agent",
    format: "markdown",
    text: "",
    thinking: "",
    toolName: "",
    toolDetail: "",
    status: "finished",
    isError: false,
    pending: false,
  };
}

function turnWorkLabel(startedAt: number, finishedAt: number): string {
  if (startedAt <= 0 || finishedAt <= startedAt) return "Worked";
  const seconds = Math.max(1, Math.round((finishedAt - startedAt) / 1000));
  return `Worked for ${seconds}s`;
}

function createChatTurn(items: TimelineItem[], running: boolean, index: number): ChatTurn {
  let userIndex = -1;
  let answerIndex = -1;
  for (let itemIndex = 0; itemIndex < items.length; itemIndex++) {
    const item = items[itemIndex];
    if (item.kind === "user" && userIndex === -1) userIndex = itemIndex;
    if (item.kind === "agent" && item.text !== "") answerIndex = itemIndex;
  }

  const fallbackId = `turn-${index}`;
  const user = userIndex >= 0 ? items[userIndex] : emptyTimelineItem(`${fallbackId}-user`);
  const answer = answerIndex >= 0 ? items[answerIndex] : emptyTimelineItem(`${fallbackId}-answer`);
  const segments: WorkSegment[] = [];
  let commentary = "";
  let tools: TimelineItem[] = [];
  let segmentIndex = 0;

  for (let itemIndex = 0; itemIndex < items.length; itemIndex++) {
    const item = items[itemIndex];
    if (item.kind === "tool_activity" || item.kind === "tool_output") {
      tools.push(item);
      continue;
    }
    if (item.kind !== "agent") continue;

    let nextCommentary = item.thinking;
    if (itemIndex !== answerIndex && item.text !== "") {
      nextCommentary = nextCommentary === "" ? item.text : `${nextCommentary}\n\n${item.text}`;
    }
    if (nextCommentary === "") continue;

    if (commentary !== "" || tools.length > 0) {
      segments.push({
        id: `${fallbackId}-work-${segmentIndex}`,
        commentary,
        tools,
      });
      segmentIndex++;
    }
    commentary = nextCommentary;
    tools = [];
  }

  if (commentary !== "" || tools.length > 0) {
    segments.push({
      id: `${fallbackId}-work-${segmentIndex}`,
      commentary,
      tools,
    });
  }

  const first = items.length > 0 ? items[0] : user;
  const last = items.length > 0 ? items[items.length - 1] : answer;
  const startedAt = user.createdAt > 0 ? user.createdAt : first.createdAt;
  const finishedAt = answer.createdAt > 0 ? answer.createdAt : last.createdAt;
  return {
    id: userIndex >= 0 ? user.id : fallbackId,
    hasUser: userIndex >= 0,
    user,
    hasWork: running || segments.length > 0,
    segments,
    hasAnswer: answerIndex >= 0,
    answer,
    running,
    startedAt,
    workLabel: turnWorkLabel(startedAt, finishedAt),
  };
}

export function buildChatTurns(messages: TimelineItem[], running: boolean): ChatTurn[] {
  const turns: ChatTurn[] = [];
  let start = 0;
  for (let index = 0; index < messages.length; index++) {
    if (messages[index].kind !== "user" || index === start) continue;
    turns.push(createChatTurn(messages.slice(start, index), false, turns.length));
    start = index;
  }
  if (start < messages.length) {
    turns.push(createChatTurn(messages.slice(start), running, turns.length));
  }
  return turns;
}
