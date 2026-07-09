import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  createEmailAccount,
  deleteEmailAccount,
  listEmailAccounts,
  type EmailAccount,
} from "../api/settings.js";

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function accountView(account: EmailAccount): { id: string; name: string; meta: string } {
  return {
    id: account.id,
    name: escapeHtml(account.name),
    meta: escapeHtml(`${account.email} - ${account.host}:${account.port}`),
  };
}

async function submitEmail(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
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

  details summary {
    color: var(--foreground);
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
  }

  details .grid {
    margin-top: 14px;
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

export function AppSettingsEmail(): Component {
  let accounts = state([] as EmailAccount[]);
  let loaded = state(false);
  let error = state("");

  onMount(() => {
    void listEmailAccounts()
      .then((items) => {
        accounts = items;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load email accounts.";
      });
  });

  const accountViews = accounts.map(accountView);

  return (
    <>
      <style>{styles}</style>
      <app-card
        title="Email accounts"
        description="Connect one or more TLS IMAP accounts. S.T.R.I.D.E. can read incoming and sent mail and save reply-all drafts. It cannot send email."
      >
        {accountViews.length > 0
          ? (
            <div class="account-list">
              {accountViews.map((account) => (
                <div class="account" key={account.id}>
                  <div>
                    <div class="name">{account.name}</div>
                    <div class="meta">{account.meta}</div>
                  </div>
                  <app-button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm("Remove this IMAP account from S.T.R.I.D.E.?")) return;
                      void deleteEmailAccount(account.id)
                        .then(() => listEmailAccounts())
                        .then((items) => {
                          accounts = items;
                          loaded = true;
                          error = "";
                        })
                        .catch(() => {
                          error = "Failed to remove email account.";
                        });
                    }}
                  >
                    Remove
                  </app-button>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No IMAP accounts yet." : "Loading accounts..."}</p>}
      </app-card>

      <app-card title="Add IMAP server" description="The connection is verified before it is saved. Credentials are encrypted at rest.">
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const form = event.target as HTMLFormElement;
            error = "";
            void submitEmail(form)
              .then(() => {
                form.reset();
                return listEmailAccounts();
              })
              .then((items) => {
                accounts = items;
                loaded = true;
                error = "";
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to add email account.";
              });
          }}
        >
          <div class="grid">
            <label>Account name<input name="name" required placeholder="Work" autocomplete="off" /></label>
            <label>Email address<input name="email" type="email" required placeholder="you@example.com" autocomplete="email" /></label>
            <label>IMAP host<input name="host" required placeholder="imap.example.com" autocomplete="off" /></label>
            <label>Port<input name="port" type="number" min="1" max="65535" value="993" required /></label>
            <label>Username<input name="username" required placeholder="you@example.com" autocomplete="username" /></label>
            <label>Password or app password<input name="password" type="password" required autocomplete="new-password" /></label>
          </div>
          <details>
            <summary>Mailbox names</summary>
            <div class="grid">
              <label>Inbox<input name="inbox_mailbox" value="INBOX" required /></label>
              <label>Sent<input name="sent_mailbox" value="Sent" required /></label>
              <label>Drafts<input name="drafts_mailbox" value="Drafts" required /></label>
            </div>
          </details>
          <div class="actions"><app-button>Add account</app-button></div>
          <p class="error">{error}</p>
        </form>
      </app-card>
    </>
  );
}
