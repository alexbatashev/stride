export interface TimelineMessage {
  id: string;
  seq: number;
  role: string;
  messageType: string;
  format: string;
  content: string;
  thinking: string;
  toolName: string;
}

export interface TimelineItem {
  id: string;
  seq: number;
  role: string;
  kind: string;
  format: string;
  text: string;
  thinking: string;
  toolName: string;
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

export function buildTimeline(messages: TimelineMessage[]): TimelineItem[] {
  return messages.map((message) => ({
    id: message.id,
    seq: message.seq,
    role: message.role,
    kind: timelineKind(message),
    format: message.format,
    text: message.format === "markdown" ? decodeLegacyText(message.content) : message.content,
    thinking: decodeLegacyText(message.thinking),
    toolName: message.toolName,
  }));
}
