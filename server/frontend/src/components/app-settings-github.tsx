import { Component, css, onMount } from "@frontiers-labs/argon";
import {
  connectGitHubPat,
  disconnectGitHub,
  getGitHubSettings,
  startGitHubAuthorize,
} from "../api/settings.js";

type GitHubHost = HTMLElement & {
  configured: boolean;
  connected: boolean;
  status: string;
  error: string;
};

async function refreshGitHub(host: GitHubHost): Promise<void> {
  try {
    const settings = await getGitHubSettings();
    host.error = "";
    host.configured = settings.configured;
    host.connected = settings.connected;
    if (settings.connected) {
      const via = settings.auth_method === "pat" ? " via personal access token" : "";
      host.status = settings.login
        ? `Connected as @${settings.login}${via}.`
        : "GitHub is connected.";
    } else if (settings.configured) {
      host.status = "GitHub is not connected.";
    } else {
      host.status = "Connect a personal access token below to enable GitHub tools.";
    }
  } catch {
    host.error = "Failed to load GitHub settings.";
  }
}

async function connectWithPat(host: GitHubHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  const token = String(data.get("token") ?? "").trim();
  host.error = "";
  if (!token) {
    host.error = "Enter a personal access token.";
    return;
  }
  try {
    await connectGitHubPat(token);
    form.reset();
    await refreshGitHub(host);
  } catch (error) {
    host.error = error instanceof Error ? error.message : "Failed to connect GitHub.";
  }
}

async function connectGitHub(host: GitHubHost): Promise<void> {
  host.error = "";
  try {
    window.location.assign(await startGitHubAuthorize());
  } catch {
    host.error = "Failed to start GitHub sign in.";
  }
}

const styles = css`
  .status-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .status,
  .muted {
    color: var(--muted-foreground);
    font-size: 13px;
  }

  form {
    display: grid;
    gap: 10px;
  }

  label {
    color: var(--foreground);
    display: grid;
    font-size: 13px;
    gap: 6px;
  }

  input {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--foreground);
    font: inherit;
    min-height: 34px;
    padding: 7px 9px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    margin: 8px 0 0;
  }

  .error:empty {
    display: none;
  }
`;

export function AppSettingsGithub({
  configured = false,
  connected = false,
  status = "Loading...",
  error = "",
}: {
  configured?: boolean;
  connected?: boolean;
  status?: string;
  error?: string;
}): Component {
  onMount(() => {
    void refreshGitHub(this);
  });

  return (
    <>
      <style>{styles}</style>
      <app-card title="GitHub" description="Give your agents the official GitHub MCP tools for repositories, issues, and pull requests. Sign in with GitHub, or paste a personal access token if your server has no GitHub app configured.">
        <div class="status-row">
          {connected
            ? <app-badge>Connected</app-badge>
            : <app-badge variant="outline">Not connected</app-badge>}
          <span class="status">{status}</span>
        </div>
        {connected
          ? (
            <div>
              <app-button
                variant="outline"
                onClick={() => {
                  void disconnectGitHub()
                    .then(() => refreshGitHub(this))
                    .catch(() => {
                      this.error = "Failed to disconnect GitHub.";
                    });
                }}
              >
                Disconnect
              </app-button>
            </div>
          )
          : (
            <>
              {configured
                ? <div><app-button onClick={() => { void connectGitHub(this); }}>Sign in with GitHub</app-button></div>
                : ""}
              <form
                onSubmit={(event: Event) => {
                  event.preventDefault();
                  void connectWithPat(this, event.target as HTMLFormElement);
                }}
              >
                <label>
                  Personal access token
                  <input
                    name="token"
                    type="password"
                    placeholder="ghp_... or github_pat_..."
                    autocomplete="off"
                  />
                </label>
                <p class="muted">
                  Create a token with the scopes your agents need (for example <code>repo</code> and <code>read:org</code>) at github.com/settings/tokens. It is encrypted at rest and forwarded only to the GitHub MCP server.
                </p>
                <div class="actions"><app-button>Connect with token</app-button></div>
              </form>
            </>
          )}
        <p class="error">{error}</p>
      </app-card>
    </>
  );
}
