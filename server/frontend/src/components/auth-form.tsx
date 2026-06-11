import { Component, css, effect, onMount, ref, state } from "@frontiers-labs/argon";
import { AuthMode, authenticate } from "../api/auth.js";
import { AppTextInput } from "./app-text-input.js";
import { AppButton } from "./app-button.js";

const styles = css`
  :host {
    display: block;
  }

  form {
    display: grid;
    gap: 16px;
  }

  .actions {
    display: grid;
    gap: 10px;
    grid-template-columns: 1fr 1fr;
    margin-top: 4px;
  }

  .error {
    background: #fff1f0;
    border: 1px solid #ffccc7;
    border-radius: 6px;
    color: #9f1d16;
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
  const submitLabel = isLogin ? "Log in" : "Register";
  const switchLabel = isLogin ? "Register" : "Log in";

  const submitButton = ref<HTMLElement>();
  const switchButton = ref<HTMLElement>();
  effect(() => {
    submitButton.current?.toggleAttribute("loading", loading);
    switchButton.current?.toggleAttribute("disabled", loading);
  });
  onMount(() => {
    // Enter inside a shadow-DOM input cannot submit this form natively;
    // app-text-input surfaces it as a composed "commit" event instead.
    const onCommit = () => void submit(this, this.shadowRoot!);
    this.shadowRoot!.addEventListener("commit", onCommit);
    return () => this.shadowRoot!.removeEventListener("commit", onCommit);
  });

  return (
    <>
      <style>{styles}</style>
      <form
        onSubmit={(event: Event) => {
          event.preventDefault();
          void submit(this, this.shadowRoot!);
        }}
      >
        <h1>{title}</h1>
        {error !== "" && <p class="error">{error}</p>}
        <AppTextInput label="Username" name="username" autocomplete="username" disabled={loading} required={true} />
        <AppTextInput
          label="Password"
          name="password"
          kind="password"
          autocomplete={isLogin ? "current-password" : "new-password"}
          disabled={loading}
          required={true}
        />
        <div class="actions">
          <AppButton
            ref={submitButton}
            onClick={(event: Event) => {
              event.preventDefault();
              void submit(this, this.shadowRoot!);
            }}
          >
            {submitLabel}
          </AppButton>
          <AppButton
            ref={switchButton}
            variant="secondary"
            onClick={() => {
              const next = this.mode === "login" ? "register" : "login";
              this.dispatchEvent(
                new CustomEvent("auth-mode-change", { bubbles: true, composed: true, detail: { mode: next } }),
              );
            }}
          >
            {switchLabel}
          </AppButton>
        </div>
      </form>
    </>
  );
}
