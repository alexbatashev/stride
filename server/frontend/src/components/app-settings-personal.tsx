import { Component, css, effect, onMount, ref, state } from "@frontiers-labs/argon";
import {
  getPersonalSettings,
  updatePersonalSettings,
} from "../api/settings.js";
import { settings } from "../stores/settings.js";

const styles = css`
  form {
    display: grid;
    gap: 20px;
  }

  label {
    color: var(--foreground);
    display: grid;
    font-size: 13px;
    font-weight: 500;
    gap: 6px;
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
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease;
    width: 100%;
  }

  input {
    height: 36px;
  }

  textarea {
    line-height: 1.5;
    min-height: 180px;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  input:disabled {
    background: var(--muted);
    color: var(--muted-foreground);
    cursor: not-allowed;
  }

  .hint,
  .status {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.45;
  }

  .actions {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .actions app-button {
    width: auto;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
  }

  .error:empty {
    display: none;
  }
`;

export function AppSettingsPersonal(): Component {
  let username = state("");
  let fullName = state("");
  let personality = state("");
  let loaded = state(false);
  let saving = state(false);
  let saved = state(false);
  let error = state("");
  const personalityArea = ref<HTMLTextAreaElement>();

  effect(() => {
    const area = personalityArea.current;
    if (area && area.value !== personality) area.value = personality;
  });

  onMount(() => {
    void getPersonalSettings()
      .then((profile) => {
        username = profile.username;
        fullName = profile.full_name;
        personality = profile.personality;
        settings.username = profile.username;
        settings.fullName = profile.full_name;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load your personal settings.";
      });
  });

  return (
    <>
      <style>{styles}</style>
      <app-settings-section
        title="Profile"
        description="Choose how your name appears in Stride. Your username remains fixed and is used to sign in."
      >
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const nextName = fullName.trim();
            if (!nextName) {
              error = "Full name cannot be empty.";
              return;
            }
            saving = true;
            saved = false;
            error = "";
            void updatePersonalSettings({
              full_name: nextName,
              personality: personality.trim(),
            })
              .then((profile) => {
                username = profile.username;
                fullName = profile.full_name;
                personality = profile.personality;
                settings.username = profile.username;
                settings.fullName = profile.full_name;
                saved = true;
              })
              .catch((reason) => {
                error = reason instanceof Error ? reason.message : "Failed to save your personal settings.";
              })
              .finally(() => {
                saving = false;
              });
          }}
        >
          <label>
            Full name
            <input
              name="full-name"
              autocomplete="name"
              value={fullName}
              disabled={!loaded || saving}
              onInput={(event: Event) => {
                fullName = (event.target as HTMLInputElement).value;
                saved = false;
              }}
            />
          </label>
          <label>
            Username
            <input name="username" value={username} disabled />
            <span class="hint">Your username is set when the account is created.</span>
          </label>
          <label>
            Personality
            <textarea
              ref={personalityArea}
              name="personality"
              placeholder="Tell Stride how you prefer to work, communicate, and make decisions."
              disabled={!loaded || saving}
              onInput={(event: Event) => {
                personality = (event.target as HTMLTextAreaElement).value;
                saved = false;
              }}
            ></textarea>
            <span class="hint">Stride includes this context when working with you. Leave it blank to use the default behavior.</span>
          </label>
          <div class="actions">
            <app-button type="submit" disabled={!loaded || saving}>{saving ? "Saving..." : "Save changes"}</app-button>
            {saved ? <span class="status">Saved.</span> : ""}
            {!loaded && !error ? <span class="status">Loading...</span> : ""}
            <span class="error">{error}</span>
          </div>
        </form>
      </app-settings-section>
    </>
  );
}
