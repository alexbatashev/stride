import { Component, css, onMount } from "@frontiers-labs/argon";
import {
  disconnectGoogle,
  getGoogleSettings,
  startGoogleAuthorize,
} from "../api/settings.js";

type GoogleHost = HTMLElement & {
  configured: boolean;
  connected: boolean;
  status: string;
  error: string;
};

async function refreshGoogle(host: GoogleHost): Promise<void> {
  try {
    const settings = await getGoogleSettings();
    host.error = "";
    host.configured = settings.configured;
    host.connected = settings.connected;
    if (!settings.configured) {
      host.status = "Google is not configured on this server.";
    } else if (settings.connected) {
      host.status = settings.email ? `Connected as ${settings.email}.` : "Google is connected.";
    } else {
      host.status = "Google is not connected.";
    }
  } catch {
    host.error = "Failed to load Google settings.";
  }
}

async function connectGoogle(host: GoogleHost): Promise<void> {
  host.error = "";
  try {
    window.location.assign(await startGoogleAuthorize());
  } catch {
    host.error = "Failed to start Google sign in.";
  }
}

const styles = css`
  .status-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .status {
    color: var(--muted-foreground);
    font-size: 13px;
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

export function AppSettingsGoogle({
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
    void refreshGoogle(this);
  });

  return (
    <>
      <style>{styles}</style>
      <app-card title="Google" description="Connect your Google account to give your agents native Gmail, Calendar, and Drive tools, and to trigger automations on new Gmail. Gmail is read and draft only — agents never send mail.">
        <div class="status-row">
          {connected
            ? <app-badge>Connected</app-badge>
            : configured
              ? <app-badge variant="outline">Not connected</app-badge>
              : <app-badge variant="secondary">Unavailable</app-badge>}
          <span class="status">{status}</span>
        </div>
        {configured
          ? (connected
            ? (
              <div>
                <app-button
                  variant="outline"
                  onClick={() => {
                    void disconnectGoogle()
                      .then(() => refreshGoogle(this))
                      .catch(() => {
                        this.error = "Failed to disconnect Google.";
                      });
                  }}
                >
                  Disconnect
                </app-button>
              </div>
            )
            : <div><app-button onClick={() => { void connectGoogle(this); }}>Sign in with Google</app-button></div>)
          : ""}
        <p class="error">{error}</p>
      </app-card>
    </>
  );
}
