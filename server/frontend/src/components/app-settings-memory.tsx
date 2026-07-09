import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  deleteMemory,
  listMemories,
  type Memory,
  type MemoryRoom,
  type MemorySettings,
  type MemoryWing,
} from "../api/settings.js";

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

type MemoryView = {
  id: string;
  title: string;
  path: string;
  summary: string;
  content: string;
  source: string;
  keywords: string;
  created: string;
  rawSearch: string;
};

type RoomView = {
  id: string;
  wing: string;
  name: string;
  description: string;
  memories: number;
};

type WingView = {
  id: string;
  name: string;
  memories: number;
  rooms: RoomView[];
};

function formatDate(seconds: number): string {
  if (!seconds) return "Unknown date";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

function memoryView(memory: Memory): MemoryView {
  const title = memory.title || memory.summary || "Untitled memory";
  const summary = memory.summary || memory.content.slice(0, 180);
  const source = memory.source || "Agent memory";
  return {
    id: memory.id,
    title: escapeHtml(title),
    path: escapeHtml(`${memory.wing} / ${memory.room}`),
    summary: escapeHtml(summary),
    content: escapeHtml(memory.content),
    source: escapeHtml(source),
    keywords: escapeHtml(memory.keywords),
    created: escapeHtml(formatDate(memory.created_at)),
    rawSearch: `${title} ${summary} ${memory.content} ${memory.wing} ${memory.room} ${memory.keywords}`.toLowerCase(),
  };
}

function roomView(room: MemoryRoom): RoomView {
  return {
    id: room.id,
    wing: escapeHtml(room.wing),
    name: escapeHtml(room.name),
    description: escapeHtml(room.description),
    memories: room.memories,
  };
}

function wingView(wing: MemoryWing, rooms: MemoryRoom[]): WingView {
  return {
    id: wing.id,
    name: escapeHtml(wing.name),
    memories: wing.memories,
    rooms: rooms.filter((room) => room.wing === wing.name).map(roomView),
  };
}

const styles = css`
  .memory-overview {
    display: grid;
    gap: 14px;
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .memory-stat {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
  }

  .memory-stat .value {
    color: var(--foreground);
    font-size: 24px;
    font-weight: 650;
    line-height: 1;
  }

  .memory-stat .label {
    color: var(--muted-foreground);
    font-size: 11px;
    letter-spacing: 0.08em;
    margin-top: 7px;
    text-transform: uppercase;
  }

  .memory-workspace {
    align-items: start;
    display: grid;
    gap: 16px;
    grid-template-columns: minmax(220px, 0.8fr) minmax(0, 1.2fr);
    margin-top: 20px;
  }

  .panels {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-width: 0;
  }

  .memory-map,
  .memory-ledger,
  .memory-detail {
    border: 1px solid var(--border);
    border-radius: 8px;
    min-width: 0;
  }

  .memory-map {
    background:
      linear-gradient(90deg, color-mix(in srgb, var(--border) 45%, transparent) 1px, transparent 1px) 0 0 / 24px 24px,
      linear-gradient(color-mix(in srgb, var(--border) 45%, transparent) 1px, transparent 1px) 0 0 / 24px 24px;
    padding: 14px;
  }

  .map-wing {
    display: grid;
    gap: 8px;
  }

  .map-wing + .map-wing {
    border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
    margin-top: 14px;
    padding-top: 14px;
  }

  .map-wing-head {
    align-items: baseline;
    display: flex;
    gap: 10px;
    justify-content: space-between;
  }

  .map-wing-name {
    color: var(--foreground);
    font-size: 13px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }

  .map-wing-count {
    color: var(--muted-foreground);
    flex: 0 0 auto;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .map-room {
    align-items: center;
    color: var(--muted-foreground);
    display: grid;
    font-size: 12px;
    gap: 8px;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    min-height: 24px;
  }

  .map-room::before {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 999px;
    content: "";
    height: 7px;
    justify-self: center;
    width: 7px;
  }

  .room-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .room-count {
    color: var(--foreground);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .status-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .memory-tools {
    display: grid;
    gap: 12px;
    margin-bottom: 14px;
  }

  .memory-search {
    position: relative;
  }

  .memory-search input {
    padding-left: 34px;
  }

  .memory-search::before {
    color: var(--muted-foreground);
    content: "⌕";
    font-size: 19px;
    left: 12px;
    line-height: 1;
    position: absolute;
    top: 8px;
  }

  input {
    background: var(--background);
    border: 1px solid var(--input);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    font-size: 14px;
    height: 36px;
    outline: none;
    padding: 8px 10px;
    width: 100%;
  }

  input:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .memory-ledger {
    overflow: hidden;
  }

  .memory-row {
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--border);
    color: inherit;
    cursor: pointer;
    display: grid;
    gap: 5px;
    padding: 12px 14px;
    text-align: left;
    width: 100%;
  }

  .memory-row:last-child {
    border-bottom: 0;
  }

  .memory-row:hover,
  .memory-row[aria-current="true"] {
    background: var(--accent);
  }

  .memory-row-title {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }

  .memory-row-path,
  .memory-row-summary,
  .memory-detail-meta,
  .memory-tags,
  .muted,
  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.45;
  }

  .memory-row-summary {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .memory-detail {
    display: grid;
    gap: 14px;
    padding: 14px;
  }

  .memory-detail-head {
    align-items: start;
    display: flex;
    gap: 14px;
    justify-content: space-between;
  }

  h3 {
    color: var(--foreground);
    font-size: 16px;
    line-height: 1.25;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .memory-detail-content {
    color: var(--foreground);
    font-size: 13px;
    line-height: 1.55;
    max-height: 280px;
    overflow: auto;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    margin: 8px 0 0;
  }

  .error:empty {
    display: none;
  }

  @media (max-width: 767px) {
    .memory-overview,
    .memory-workspace {
      grid-template-columns: 1fr;
    }
  }
`;

export function AppSettingsMemory(): Component {
  let wings = state([] as MemoryWing[]);
  let rooms = state([] as MemoryRoom[]);
  let memories = state([] as Memory[]);
  let loaded = state(false);
  let error = state("");
  let query = state("");
  let selectedId = state("");

  onMount(() => {
    void listMemories()
      .then((settings: MemorySettings) => {
        wings = settings.wings;
        rooms = settings.rooms;
        memories = settings.memories;
        loaded = true;
        error = "";
        if (selectedId && !settings.memories.some((memory) => memory.id === selectedId)) {
          selectedId = settings.memories[0]?.id ?? "";
        }
      })
      .catch(() => {
        error = "Failed to load memories.";
      });
  });

  const memoryViews = memories.map(memoryView);
  const trimmed = query.trim().toLowerCase();
  const filteredMemories = trimmed
    ? memoryViews.filter((memory) => memory.rawSearch.includes(trimmed))
    : memoryViews;
  const selectedMemory = filteredMemories.find((memory) => memory.id === selectedId)
    ?? filteredMemories[0]
    ?? null;
  const memoryWingViews = wings.map((wing) => wingView(wing, rooms));

  return (
    <>
      <style>{styles}</style>
      <app-card
        title="Memory palace"
        description="Review durable memories your agents can recall across threads. New memories are still created by asking the agent to remember something."
      >
        <div class="memory-overview">
          <div class="memory-stat">
            <div class="value">{wings.length}</div>
            <div class="label">Wings</div>
          </div>
          <div class="memory-stat">
            <div class="value">{rooms.length}</div>
            <div class="label">Rooms</div>
          </div>
          <div class="memory-stat">
            <div class="value">{memories.length}</div>
            <div class="label">Memories</div>
          </div>
        </div>
        <div class="status-row">
          <app-button
            variant="outline"
            size="sm"
            onClick={() => {
              void listMemories()
                .then((settings: MemorySettings) => {
                  wings = settings.wings;
                  rooms = settings.rooms;
                  memories = settings.memories;
                  loaded = true;
                  error = "";
                  if (selectedId && !settings.memories.some((memory) => memory.id === selectedId)) {
                    selectedId = settings.memories[0]?.id ?? "";
                  }
                })
                .catch(() => {
                  error = "Failed to load memories.";
                });
            }}
          >
            Refresh
          </app-button>
          <span class="muted">{loaded ? "Showing saved memory structure." : "Loading memories..."}</span>
        </div>
        <p class="error">{error}</p>
      </app-card>

      <div class="memory-workspace">
        <app-card title="Palace map" description="Wings hold rooms; rooms hold individual memories. Empty rooms stay visible so the structure is easy to audit.">
          {wings.length > 0
            ? (
              <div class="memory-map">
                {memoryWingViews.map((wing) => (
                  <div class="map-wing" key={wing.id}>
                    <div class="map-wing-head">
                      <div class="map-wing-name">{wing.name}</div>
                      <div class="map-wing-count">{wing.memories} memories</div>
                    </div>
                    {wing.rooms.length > 0
                      ? wing.rooms.map((room) => (
                        <div class="map-room" key={room.id} title={room.description}>
                          <span class="room-name">{room.name}</span>
                          <span class="room-count">{room.memories}</span>
                        </div>
                      )).join("")
                      : <p class="muted">No rooms yet.</p>}
                  </div>
                )).join("")}
              </div>
            )
            : <p class="muted">{loaded ? "No memory wings yet." : "Loading palace map..."}</p>}
        </app-card>

        <div class="panels">
          <app-card title="Memory ledger" description="Search titles, summaries, rooms, and original contents. Removing a memory deletes the saved drawer and its search card.">
            <div class="memory-tools">
              <label class="memory-search">
                <input
                  name="memory-query"
                  value={query}
                  placeholder="Search memories"
                  aria-label="Search memories"
                  autocomplete="off"
                  onInput={(event: Event) => {
                    query = (event.target as HTMLInputElement).value;
                  }}
                />
              </label>
              <span class="hint">{filteredMemories.length} of {memories.length} memories shown</span>
            </div>
            {filteredMemories.length > 0
              ? (
                <div class="memory-ledger">
                  {filteredMemories.map((memory) => (
                    <button
                      type="button"
                      class="memory-row"
                      aria-current={selectedMemory?.id === memory.id ? "true" : "false"}
                      onClick={() => { selectedId = memory.id; }}
                    >
                      <span class="memory-row-title">{memory.title}</span>
                      <span class="memory-row-path">{memory.path}</span>
                      <span class="memory-row-summary">{memory.summary}</span>
                    </button>
                  )).join("")}
                </div>
              )
              : <p class="muted">{loaded ? "No memories match this search." : "Loading memories..."}</p>}
          </app-card>

          <app-card title="Selected memory" description="Inspect the stored summary, original content, source, and search keywords before removing anything.">
            {selectedMemory
              ? (
                <div class="memory-detail">
                  <div class="memory-detail-head">
                    <div>
                      <h3>{selectedMemory.title}</h3>
                      <div class="memory-detail-meta">{selectedMemory.path} - {selectedMemory.created}</div>
                    </div>
                    <app-button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        if (!window.confirm("Remove this memory? This cannot be undone.")) return;
                        void deleteMemory(selectedMemory.id)
                          .then(() => listMemories())
                          .then((settings: MemorySettings) => {
                            wings = settings.wings;
                            rooms = settings.rooms;
                            memories = settings.memories;
                            loaded = true;
                            error = "";
                            if (selectedId && !settings.memories.some((memory) => memory.id === selectedId)) {
                              selectedId = settings.memories[0]?.id ?? "";
                            }
                          })
                          .catch(() => {
                            error = "Failed to remove memory.";
                          });
                      }}
                    >
                      Remove
                    </app-button>
                  </div>
                  <p class="muted">{selectedMemory.summary}</p>
                  <div class="memory-detail-content">{selectedMemory.content}</div>
                  <div class="memory-tags">Source: {selectedMemory.source}</div>
                  {selectedMemory.keywords ? <div class="memory-tags">Keywords: {selectedMemory.keywords}</div> : ""}
                </div>
              )
              : <p class="muted">{loaded ? "Select a memory to inspect it." : "Loading selected memory..."}</p>}
          </app-card>
        </div>
      </div>
    </>
  );
}
