import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  createMcpServer,
  deleteMcpServer,
  listMcpServers,
  type McpServer,
} from "../api/settings.js";

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function serverView(server: McpServer): { id: string; name: string; meta: string } {
  const headers = [server.has_authorization ? "Authorization" : "", ...server.header_names].filter(Boolean);
  return {
    id: server.id,
    name: escapeHtml(server.name),
    meta: escapeHtml(headers.length > 0 ? `${server.url} - headers: ${headers.join(", ")}` : server.url),
  };
}

async function submitMcp(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  await createMcpServer({
    name: String(data.get("name") ?? "").trim(),
    url: String(data.get("url") ?? "").trim(),
    bearer_token: String(data.get("bearer_token") ?? ""),
    headers_json: String(data.get("headers_json") ?? "").trim(),
    enabled: true,
  });
}

const styles = css`
  .account-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .account {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 10px;
    display: flex;
    gap: 16px;
    justify-content: space-between;
    padding: 12px 14px;
  }

  .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
  }

  .meta,
  .muted {
    color: var(--muted-foreground);
    font-size: 13px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  form {
    display: grid;
    gap: 14px;
  }

  .grid {
    display: grid;
    gap: 14px;
    grid-template-columns: 1fr 1fr;
  }

  label {
    color: var(--foreground);
    display: grid;
    font-size: 13px;
    font-weight: 500;
    gap: 6px;
  }

  label.full {
    grid-column: 1 / -1;
  }

  input,
  textarea {
    background: var(--background);
    border: 1px solid var(--input);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    font-size: 14px;
    outline: none;
    padding: 8px 10px;
    width: 100%;
  }

  input {
    height: 36px;
  }

  textarea {
    min-height: 84px;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .actions app-button,
  .account app-button {
    width: auto;
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
    .grid {
      grid-template-columns: 1fr;
    }
  }
`;

export function AppSettingsMcp(): Component {
  let servers = state([] as McpServer[]);
  let loaded = state(false);
  let error = state("");

  onMount(() => {
    void listMcpServers()
      .then((items) => {
        servers = items;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load MCP servers.";
      });
  });

  const serverViews = servers.map(serverView);

  return (
    <>
      <style>{styles}</style>
      <app-card
        title="MCP servers"
        description="Add remote HTTP MCP servers for your agents. Tools from these servers load alongside the global MCP servers."
      >
        {serverViews.length > 0
          ? (
            <div class="account-list">
              {serverViews.map((server) => (
                <div class="account" key={server.id}>
                  <div>
                    <div class="name">{server.name}</div>
                    <div class="meta">{server.meta}</div>
                  </div>
                  <app-button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm("Remove this MCP server from S.T.R.I.D.E.?")) return;
                      void deleteMcpServer(server.id)
                        .then(() => listMcpServers())
                        .then((items) => {
                          servers = items;
                          loaded = true;
                          error = "";
                        })
                        .catch(() => {
                          error = "Failed to remove MCP server.";
                        });
                    }}
                  >
                    Remove
                  </app-button>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No custom MCP servers yet." : "Loading servers..."}</p>}
      </app-card>

      <app-card title="Add MCP server" description="Only Streamable HTTP MCP servers are supported here. Authorization values are stored but not shown again.">
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const form = event.target as HTMLFormElement;
            error = "";
            void submitMcp(form)
              .then(() => {
                form.reset();
                return listMcpServers();
              })
              .then((items) => {
                servers = items;
                loaded = true;
                error = "";
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to add MCP server.";
              });
          }}
        >
          <div class="grid">
            <label>Name<input name="name" required placeholder="deepwiki" autocomplete="off" pattern="[A-Za-z][A-Za-z0-9_]{1,47}" /></label>
            <label>URL<input name="url" type="url" required placeholder="https://mcp.example.com/mcp" autocomplete="off" /></label>
          </div>
          <label class="full">Bearer token<input name="bearer_token" type="password" autocomplete="new-password" /></label>
          <label class="full">Headers JSON<textarea name="headers_json" placeholder='{"X-Tenant":"acme"}'></textarea></label>
          <div class="actions"><app-button>Add server</app-button></div>
          <p class="error">{error}</p>
        </form>
      </app-card>
    </>
  );
}
