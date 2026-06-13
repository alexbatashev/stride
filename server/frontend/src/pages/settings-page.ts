import { logout } from "../api/auth.js";
import {
	createTelegramConnectCode,
	disconnectTelegram,
	getTelegramSettings,
	type TelegramSettings,
} from "../api/settings.js";

const root = document.querySelector<HTMLElement>("#settings-page");

class SettingsPage {
	private readonly statusEl: HTMLElement;
	private readonly codeEl: HTMLElement;
	private readonly helpEl: HTMLElement;
	private readonly linkEl: HTMLAnchorElement;
	private readonly errorEl: HTMLElement;

	constructor(private readonly root: HTMLElement) {
		this.statusEl = this.mustQuery("[data-telegram-status]");
		this.codeEl = this.mustQuery("[data-connect-code]");
		this.helpEl = this.mustQuery("[data-connect-help]");
		this.linkEl = this.mustQuery("[data-connect-link]");
		this.errorEl = this.mustQuery("[data-error]");
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

	private bindEvents() {
		this.root.querySelector('[data-action="connect-code"]')?.addEventListener("click", () => {
			void this.createCode();
		});
		this.root.querySelector('[data-action="disconnect"]')?.addEventListener("click", () => {
			void this.disconnect();
		});
		this.root.querySelector("app-sidebar")?.addEventListener("logout", () => void this.onLogout());
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
		this.codeEl.style.display = "none";
		this.codeEl.textContent = "";
		this.helpEl.textContent = "";
		this.linkEl.style.display = "none";
		this.linkEl.removeAttribute("href");

		if (!settings.bot_configured) {
			this.statusEl.textContent = "Telegram bot is not configured on this server.";
			return;
		}

		if (settings.connected) {
			const name = settings.username
				? `@${settings.username}`
				: [settings.first_name, settings.last_name].filter(Boolean).join(" ");
			this.statusEl.textContent = name ? `Connected as ${name}.` : "Telegram is connected.";
		} else {
			const bot = settings.bot_username ? ` Bot: @${settings.bot_username}.` : "";
			this.statusEl.textContent = `Telegram is not connected.${bot}`;
		}
	}

	private async createCode() {
		try {
			const result = await createTelegramConnectCode();
			this.codeEl.textContent = result.code;
			this.codeEl.style.display = "block";
			if (result.start_url) {
				this.linkEl.href = result.start_url;
				this.linkEl.style.display = "inline-flex";
				this.helpEl.textContent = "Open Telegram and press Start. Code expires in 10 minutes.";
			} else {
				this.helpEl.textContent = "Send /start CODE to your Friday bot. Code expires in 10 minutes.";
			}
			this.setError("");
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to create code.");
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

	private async onLogout() {
		await logout();
		window.location.href = "/auth/login";
	}

	private setError(message: string) {
		this.errorEl.textContent = message;
	}
}

if (root) {
	new SettingsPage(root);
}
