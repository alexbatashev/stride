import { Component, css, effect, ref, state } from "@frontiers-labs/argon";
import { AuthMode, authenticate } from "../api/auth.js";
import { AppTextInput } from "./app-text-input.js";
import { AppButton } from "./app-button.js";

const styles = css`
  :host {
    display: block;
    width: 100%;
  }

  .card {
    background: var(--card, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgb(0 0 0 / 5%);
    box-sizing: border-box;
    padding: 28px 24px;
  }

  .header {
    display: grid;
    gap: 6px;
    margin-bottom: 22px;
  }

  .header h1 {
    color: var(--card-foreground, var(--foreground));
    font-size: 22px;
    font-weight: 600;
    line-height: 1.2;
    margin: 0;
  }

  .header p {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.4;
    margin: 0;
  }

  form {
    display: grid;
    gap: 16px;
  }

  app-button.submit {
    display: block;
    margin-top: 4px;
    width: 100%;
  }

  .switch {
    color: var(--muted-foreground);
    font-size: 14px;
    margin: 8px 0 0;
    text-align: center;
  }

  .switch a {
    color: var(--foreground);
    text-decoration: underline;
    text-underline-offset: 4px;
  }

  .error {
    background: var(--destructive-muted, #fff1f0);
    border-radius: 8px;
    color: var(--destructive, #9f1d16);
    font-size: 14px;
    margin: 0;
    padding: 10px 12px;
  }
`;

async function submit(host: HTMLElement & { mode?: string }, root: ShadowRoot): Promise<void> {
  const form = host as HTMLElement & { error: string; loading: boolean };
  if (form.loading) return;

  const username = root.querySelector<HTMLElement & { value: string }>('app-text-input[data-name="username"]')!.value;
  const password = root.querySelector<HTMLElement & { value: string }>('app-text-input[data-name="password"]')!.value;

  form.loading = true;
  form.error = "";
  try {
    await authenticate((host.mode ?? "login") as AuthMode, username, password);
    host.dispatchEvent(new CustomEvent("auth-success", { bubbles: true, composed: true }));
  } catch (error) {
    form.error = error instanceof Error ? error.message : "Auth request failed.";
  } finally {
    form.loading = false;
  }
}

export function AuthForm({
  mode = "login",
  error = "",
  loading = false,
}: {
  mode?: string;
  error?: string;
  loading?: boolean;
}): Component {
  const isLogin = mode === "login";
  const title = isLogin ? "Log in" : "Create account";
  const description = isLogin
    ? "Enter your credentials to access S.T.R.I.D.E."
    : "Create an account to get started.";
  const submitLabel = isLogin ? "Log in" : "Register";
  const switchPrompt = isLogin ? "Don't have an account? " : "Already have an account? ";
  const switchLabel = isLogin ? "Register" : "Log in";
  const switchHref = isLogin ? "/auth/register" : "/auth/login";

  const submitButton = ref<HTMLElement>();
  effect(() => {
    submitButton.current?.toggleAttribute("loading", loading);
  });
  return (
    <>
      <style>{styles}</style>
      <div class="card">
        <div class="header">
          <h1>{title}</h1>
          <p>{description}</p>
        </div>
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            void submit(this, this.shadowRoot!);
          }}
        >
          {error !== "" && <p class="error">{error}</p>}
          <AppTextInput
            label="Username"
            name="username"
            autocomplete="username"
            disabled={loading}
            required={true}
            on:commit={() => void submit(this, this.shadowRoot!)}
          />
          <AppTextInput
            label="Password"
            name="password"
            kind="password"
            autocomplete={isLogin ? "current-password" : "new-password"}
            disabled={loading}
            required={true}
            on:commit={() => void submit(this, this.shadowRoot!)}
          />
          <AppButton
            class="submit"
            ref={submitButton}
            onClick={(event: Event) => {
              event.preventDefault();
              void submit(this, this.shadowRoot!);
            }}
          >
            {submitLabel}
          </AppButton>
          <p class="switch">
            {switchPrompt}
            <a href={switchHref}>{switchLabel}</a>
          </p>
        </form>
      </div>
    </>
  );
}
