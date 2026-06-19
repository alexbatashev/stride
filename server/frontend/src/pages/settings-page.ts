import {
	disconnectTelegram,
	getTelegramSettings,
	loginTelegram,
	type TelegramAuthData,
	type TelegramSettings,
} from "../api/settings.js";
import { bindSidebar } from "./sidebar.js";

const root = document.querySelector<HTMLElement>("#settings-page");
const TELEGRAM_AUTH_CALLBACK = "onTelegramAuth";

class SettingsPage {
	private readonly statusEl: HTMLElement;
	private readonly widgetEl: HTMLElement;
	private readonly disconnectEl: HTMLElement;
	private readonly errorEl: HTMLElement;
	private renderedBot: string | null = null;

	constructor(private readonly root: HTMLElement) {
		this.statusEl = this.mustQuery("[data-telegram-status]");
		this.widgetEl = this.mustQuery("[data-telegram-widget]");
		this.disconnectEl = this.mustQuery('[data-action="disconnect"]');
		this.errorEl = this.mustQuery("[data-error]");
		this.exposeAuthCallback();
		this.bindEvents();
		void this.refresh();
	}

	private mustQuery<T extends Element>(selector: string): T {
		const element = this.root.querySelector<T>(selector);
		if (!element) {
			throw new Error(`Missing ${selector}`);
		}
		return element;
	}

	private exposeAuthCallback() {
		const w = window as unknown as Record<string, unknown>;
		w[TELEGRAM_AUTH_CALLBACK] = (user: TelegramAuthData) => {
			void this.handleAuth(user);
		};
	}

	private bindEvents() {
		this.disconnectEl.addEventListener("click", () => {
			void this.disconnect();
		});
		const sidebar = this.root.querySelector<HTMLElement>("app-sidebar");
		if (sidebar) {
			bindSidebar(sidebar);
		}
	}

	private async refresh() {
		try {
			this.render(await getTelegramSettings());
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to load Telegram settings.");
		}
	}

	private render(settings: TelegramSettings) {
		this.setError("");

		if (!settings.bot_configured) {
			this.statusEl.textContent = "Telegram bot is not configured on this server.";
			this.clearWidget();
			this.disconnectEl.style.display = "none";
			return;
		}

		if (settings.connected) {
			const name = settings.username
				? `@${settings.username}`
				: [settings.first_name, settings.last_name].filter(Boolean).join(" ");
			this.statusEl.textContent = name ? `Connected as ${name}.` : "Telegram is connected.";
			this.clearWidget();
			this.disconnectEl.style.display = "inline-flex";
			return;
		}

		this.statusEl.textContent = "Telegram is not connected.";
		this.disconnectEl.style.display = "none";
		if (settings.bot_username) {
			this.renderWidget(settings.bot_username);
		} else {
			this.clearWidget();
			this.statusEl.textContent =
				"Telegram bot username is unavailable, so the login button cannot be shown.";
		}
	}

	private renderWidget(botUsername: string) {
		if (this.renderedBot === botUsername) {
			return;
		}
		this.renderedBot = botUsername;
		this.widgetEl.replaceChildren();
		const script = document.createElement("script");
		script.async = true;
		script.src = "https://telegram.org/js/telegram-widget.js?22";
		script.setAttribute("data-telegram-login", botUsername);
		script.setAttribute("data-size", "large");
		script.setAttribute("data-request-access", "write");
		script.setAttribute("data-onauth", `${TELEGRAM_AUTH_CALLBACK}(user)`);
		this.widgetEl.appendChild(script);
	}

	private clearWidget() {
		this.renderedBot = null;
		this.widgetEl.replaceChildren();
	}

	private async handleAuth(user: TelegramAuthData) {
		try {
			await loginTelegram(user);
			await this.refresh();
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to connect Telegram.");
		}
	}

	private async disconnect() {
		try {
			await disconnectTelegram();
			await this.refresh();
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to disconnect Telegram.");
		}
	}

	private setError(message: string) {
		this.errorEl.textContent = message;
	}
}

if (root) {
	new SettingsPage(root);
}
