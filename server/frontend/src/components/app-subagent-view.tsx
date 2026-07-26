import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import { buildChatTurns, buildTimeline } from "../shared/timeline.js";
import type { TimelineMessage } from "../shared/timeline.js";
import { sidePanel } from "../stores/side-panel.js";
import { threadStream, type Subagent } from "../stores/thread-stream.js";
import { buildSubagentTimeline } from "./chat-timeline.js";
import { AppChatView } from "./app-chat-view.js";
import { IconChevronLeft } from "./icons/chevron-left.js";
import { loadSubagentTranscript, loadSubagents } from "./subagent-data.js";

const styles = css`
  :host { display: block; height: 100%; min-height: 0; }
  .view { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .detail-header { align-items: center; border-bottom: 1px solid var(--border); display: flex; gap: 8px; min-height: 44px; padding: 0 12px; }
  .back { align-items: center; background: transparent; border: 0; border-radius: 6px; color: var(--muted-foreground); cursor: pointer; display: inline-flex; height: 28px; justify-content: center; padding: 0; width: 28px; }
  .back:hover { background: var(--muted); color: var(--foreground); }
  .back svg { height: 16px; width: 16px; }
  .detail-title { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .list { overflow: auto; padding: 8px; }
  .row { align-items: center; background: transparent; border: 0; border-radius: 7px; color: inherit; cursor: pointer; display: grid; gap: 2px 8px; grid-template-columns: minmax(0, 1fr) auto; padding: 9px 10px; text-align: left; width: 100%; }
  .row:hover { background: var(--muted); }
  .title { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--muted-foreground); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { align-items: center; color: var(--muted-foreground); display: inline-flex; grid-row: 1 / span 2; grid-column: 2; }
  .status svg { height: 13px; width: 13px; }
  .pulse { background: var(--primary); border-radius: 999px; height: 7px; width: 7px; animation: pulse 1.4s ease-in-out infinite; }
  .empty { color: var(--muted-foreground); font-size: 13px; line-height: 1.5; padding: 32px 20px; text-align: center; }
  .result { color: var(--foreground); font-size: 14px; line-height: 1.6; overflow: auto; padding: 16px; white-space: pre-wrap; }
  app-chat-view { flex: 1; min-height: 0; }
  @keyframes pulse { 50% { opacity: .35; transform: scale(.8); } }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } }
`;

export function AppSubagentView({
  threadId = "",
  active = false,
  agents = [],
  selectedKey = "",
  refreshToken = 0,
  transcript = [],
}: {
  threadId?: string;
  active?: boolean;
  agents?: Subagent[];
  selectedKey?: string;
  refreshToken?: number;
  transcript?: TimelineMessage[];
}): Component {
  onMount(() => {
    const updateTranscript = (event: Event) => {
      const item = (event as CustomEvent<{ item: TimelineMessage }>).detail.item;
      this.transcript = [...this.transcript.filter((candidate: TimelineMessage) => candidate.id !== item.id), item]
        .sort((left: TimelineMessage, right: TimelineMessage) => left.seq - right.seq);
    };
    this.addEventListener("transcript-update", updateTranscript);
    return () => this.removeEventListener("transcript-update", updateTranscript);
  });

  effect(() => {
    if (!active || !threadId || this._agentsLoadedThread === threadId) return;
    this._agentsLoadedThread = threadId;
    void loadSubagents(threadId).then(() => {
      this.agents = [...threadStream.subagents];
    });
  });

  const selected = agents.find((agent) => agent.agentPath === selectedKey);
  effect(() => {
    const selectedAgent = agents.find((agent) => agent.agentPath === selectedKey);
    if (!active || !threadId || !selectedAgent || this._loadedAgent === selectedAgent.agentPath) return;
    this._loadedAgent = selectedAgent.agentPath;
    this._loadingAgent = selectedAgent.agentPath;
    void loadSubagentTranscript(threadId, selectedAgent.id, selectedAgent.agentPath).finally(() => {
      this._loadingAgent = "";
      this.transcript = buildSubagentTimeline(selectedAgent.agentPath);
      this.refreshToken = refreshToken + 1;
    });
  });

  const sortedAgents = [...agents]
    .sort((left, right) => right.createdAt - left.createdAt)
    .map((agent) => ({
      ...agent,
      indent: `padding-left:${10 + Math.max(0, agent.agentPath.split("/").length - 1) * 16}px`,
      summary: agent.finished && agent.result ? ` · ${agent.result.split("\n")[0].slice(0, 64)}` : "",
      statusLabel: agent.finished ? "Finished" : "Running",
      statusClass: agent.finished ? "done" : "pulse",
      statusGlyph: agent.finished ? "✓" : "",
    }));
  const turns = selected ? buildChatTurns(buildTimeline(transcript), !selected.finished) : [];
  const emptyTitle = this._loadingAgent ? "Loading transcript" : "No transcript available";
  const emptyDescription = !this._loadingAgent && selected?.result ? selected.result : "";
  return <><style>{styles}</style><div class="view">
    {selected ? <>
      <div class="detail-header"><button class="back" type="button" aria-label="Back to subagents" onClick={() => { sidePanel.selectedSubagent = ""; this.selectedKey = ""; this._loadedAgent = ""; }}><IconChevronLeft /></button><span class="detail-title">{selected.name}</span></div>
      {turns.length > 0 ? <AppChatView turns={turns} emptyTitle={emptyTitle} emptyDescription={emptyDescription} /> : selected.result ? <div class="result">{selected.result}</div> : <AppChatView turns={turns} emptyTitle={emptyTitle} emptyDescription={emptyDescription} />}
    </> : sortedAgents.length === 0 ? <div class="empty">No subagents yet.<br />Subagents appear here when a task delegates work.</div> : <div class="list">
      {sortedAgents.map((agent) => <button key={agent.id} class="row" type="button" style={agent.indent} onClick={() => { sidePanel.selectedSubagent = agent.agentPath; this._loadedAgent = ""; this.selectedKey = agent.agentPath; }}>
        <span class="title">{agent.name}</span><span class="meta">{agent.model}{agent.summary}</span><span class="status" title={agent.statusLabel}><span class={agent.statusClass}>{agent.statusGlyph}</span></span>
      </button>)}
    </div>}
  </div></>;
}
