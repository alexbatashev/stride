import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import {
  disconnectTelegram,
  getTelegramSettings,
  loginTelegram,
  type TelegramAuthData,
} from "../api/settings.js";

type TelegramHost = HTMLElement & {
  configured: boolean;
  connected: boolean;
  status: string;
  botUsername: string;
  error: string;
};

async function refreshTelegram(host: TelegramHost): Promise<void> {
  try {
    const settings = await getTelegramSettings();
    host.error = "";
    host.configured = settings.bot_configured;
    host.connected = settings.connected;
    host.botUsername = settings.bot_username ?? "";
    if (!settings.bot_configured) {
      host.status = "Telegram bot is not configured on this server.";
    } else if (settings.connected) {
      const name = settings.username
        ? `@${settings.username}`
        : [settings.first_name, settings.last_name].filter(Boolean).join(" ");
      host.status = name ? `Connected as ${name}.` : "Telegram is connected.";
    } else if (settings.bot_username) {
      host.status = "Telegram is not connected.";
    } else {
      host.status = "Telegram bot username is unavailable, so the login button cannot be shown.";
    }
  } catch {
    host.error = "Failed to load Telegram settings.";
  }
}

async function handleAuth(host: TelegramHost, user: TelegramAuthData): Promise<void> {
  try {
    await loginTelegram(user);
    await refreshTelegram(host);
  } catch {
    host.error = "Failed to connect Telegram.";
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

  .tg-widget {
    margin-top: 10px;
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

export function AppSettingsTelegram({
  configured = false,
  connected = false,
  status = "Loading...",
  botUsername = "",
  error = "",
}: {
  configured?: boolean;
  connected?: boolean;
  status?: string;
  botUsername?: string;
  error?: string;
}): Component {
  onMount(() => {
    (window as unknown as Record<string, unknown>).onTelegramAuth = (user: TelegramAuthData) => {
      void handleAuth(this, user);
    };
    void refreshTelegram(this);
  });

  // Telegram's widget script must live in light DOM so it can find itself in
  // document order. The slot keeps the injected iframe visually inside the card.
  effect(() => {
    const show = configured && !connected && botUsername;
    const existing = this.querySelector<HTMLElement>(":scope > [data-tg-widget]");
    if (!show) {
      existing?.remove();
      return;
    }
    if (existing?.dataset.bot === botUsername) return;
    existing?.remove();
    const container = document.createElement("div");
    container.dataset.tgWidget = "";
    container.dataset.bot = botUsername;
    container.setAttribute("slot", "tg-widget");
    const script = document.createElement("script");
    script.async = true;
    script.src = "https://telegram.org/js/telegram-widget.js?22";
    script.setAttribute("data-telegram-login", botUsername);
    script.setAttribute("data-size", "large");
    script.setAttribute("data-request-access", "write");
    script.setAttribute("data-onauth", "onTelegramAuth(user)");
    container.appendChild(script);
    this.appendChild(container);
  });

  return (
    <>
      <style>{styles}</style>
      <app-settings-section title="Telegram" description="Connect your Telegram account with the S.T.R.I.D.E. bot.">
        <div class="status-row">
          {connected
            ? <app-badge>Connected</app-badge>
            : configured
              ? <app-badge variant="outline">Not connected</app-badge>
              : <app-badge variant="secondary">Unavailable</app-badge>}
          <span class="status">{status}</span>
        </div>
        <div class="tg-widget"><slot name="tg-widget"></slot></div>
        {connected
          ? (
            <div>
              <app-button
                variant="outline"
                onClick={() => {
                  void disconnectTelegram()
                    .then(() => refreshTelegram(this))
                    .catch(() => {
                      this.error = "Failed to disconnect Telegram.";
                    });
                }}
              >
                Disconnect
              </app-button>
            </div>
          )
          : ""}
        <p class="error">{error}</p>
      </app-settings-section>
    </>
  );
}
