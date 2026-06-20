import {
	disconnectTelegram,
	createEmailAccount,
	deleteEmailAccount,
	getTelegramSettings,
	listEmailAccounts,
	loginTelegram,
	type TelegramAuthData,
	type TelegramSettings,
	type EmailAccount,
} from "../api/settings.js";
import { bindSidebar } from "./sidebar.js";

const root = document.querySelector<HTMLElement>("#settings-page");
const TELEGRAM_AUTH_CALLBACK = "onTelegramAuth";

class SettingsPage {
	private readonly statusEl: HTMLElement;
	private readonly widgetEl: HTMLElement;
	private readonly disconnectEl: HTMLElement;
	private readonly errorEl: HTMLElement;
	private readonly emailListEl: HTMLElement;
	private readonly emailEmptyEl: HTMLElement;
	private readonly emailFormEl: HTMLFormElement;
	private readonly emailErrorEl: HTMLElement;
	private renderedBot: string | null = null;

	constructor(private readonly root: HTMLElement) {
		this.statusEl = this.mustQuery("[data-telegram-status]");
		this.widgetEl = this.mustQuery("[data-telegram-widget]");
		this.disconnectEl = this.mustQuery('[data-action="disconnect"]');
		this.errorEl = this.mustQuery("[data-telegram-error]");
		this.emailListEl = this.mustQuery("[data-email-list]");
		this.emailEmptyEl = this.mustQuery("[data-email-empty]");
		this.emailFormEl = this.mustQuery("[data-email-form]");
		this.emailErrorEl = this.mustQuery("[data-email-error]");
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
		this.emailFormEl.addEventListener("submit", (event) => {
			event.preventDefault();
			void this.addEmailAccount();
		});
		this.emailListEl.addEventListener("click", (event) => {
			const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-email-delete]");
			if (button?.dataset.emailDelete) void this.removeEmailAccount(button.dataset.emailDelete);
		});
		const sidebar = this.root.querySelector<HTMLElement>("app-sidebar");
		if (sidebar) {
			bindSidebar(sidebar);
		}
	}

	private async refresh() {
		void this.refreshEmailAccounts();
		try {
			this.render(await getTelegramSettings());
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to load Telegram settings.");
		}
	}

	private async refreshEmailAccounts() {
		try {
			this.renderEmailAccounts(await listEmailAccounts());
			this.setEmailError("");
		} catch (error) {
			this.setEmailError(error instanceof Error ? error.message : "Failed to load email accounts.");
		}
	}

	private renderEmailAccounts(accounts: EmailAccount[]) {
		this.emailListEl.replaceChildren(...accounts.map((account) => {
			const card = document.createElement("article");
			card.className = "email-account";
			const details = document.createElement("div");
			const title = document.createElement("strong");
			title.textContent = account.name;
			const meta = document.createElement("span");
			meta.textContent = `${account.email} · ${account.host}:${account.port}`;
			details.append(title, meta);
			const remove = document.createElement("button");
			remove.type = "button";
			remove.className = "danger-button";
			remove.dataset.emailDelete = account.id;
			remove.textContent = "Remove";
			card.append(details, remove);
			return card;
		}));
		this.emailEmptyEl.style.display = accounts.length === 0 ? "block" : "none";
	}

	private async addEmailAccount() {
		const data = new FormData(this.emailFormEl);
		const submit = this.emailFormEl.querySelector<HTMLButtonElement>('button[type="submit"]');
		if (submit) submit.disabled = true;
		this.setEmailError("");
		try {
			await createEmailAccount({
				name: String(data.get("name") ?? "").trim(),
				email: String(data.get("email") ?? "").trim(),
				host: String(data.get("host") ?? "").trim(),
				port: Number(data.get("port") ?? 993),
				username: String(data.get("username") ?? "").trim(),
				password: String(data.get("password") ?? ""),
				inbox_mailbox: String(data.get("inbox_mailbox") ?? "INBOX").trim(),
				sent_mailbox: String(data.get("sent_mailbox") ?? "Sent").trim(),
				drafts_mailbox: String(data.get("drafts_mailbox") ?? "Drafts").trim(),
			});
			this.emailFormEl.reset();
			(this.emailFormEl.elements.namedItem("port") as HTMLInputElement).value = "993";
			await this.refreshEmailAccounts();
		} catch (error) {
			this.setEmailError(error instanceof Error ? error.message : "Failed to add email account.");
		} finally {
			if (submit) submit.disabled = false;
		}
	}

	private async removeEmailAccount(id: string) {
		if (!window.confirm("Remove this IMAP account from Friday?")) return;
		try {
			await deleteEmailAccount(id);
			await this.refreshEmailAccounts();
		} catch (error) {
			this.setEmailError(error instanceof Error ? error.message : "Failed to remove email account.");
		}
	}

	private setEmailError(message: string) {
		this.emailErrorEl.textContent = message;
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
