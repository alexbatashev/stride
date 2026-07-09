import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  createWritableDir,
  deleteWritableDir,
  listWritableDirs,
  type WritableDir,
} from "../api/settings.js";

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function dirView(dir: WritableDir): { id: string; name: string; meta: string } {
  return {
    id: dir.id,
    name: escapeHtml(`/${dir.path}`),
    meta: "Writable by your agents, including every subdirectory.",
  };
}

async function submitWritableDir(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  await createWritableDir(String(data.get("path") ?? "").trim());
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
`;

export function AppSettingsFiles(): Component {
  let dirs = state([] as WritableDir[]);
  let loaded = state(false);
  let error = state("");

  onMount(() => {
    void listWritableDirs()
      .then((items) => {
        dirs = items;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load writable directories.";
      });
  });

  const dirViews = dirs.map(dirView);

  return (
    <>
      <style>{styles}</style>
      <app-card
        title="Writable folders"
        description="By default your agents may only write inside a thread's workspace or its project folder. Add personal folders here to let agents create and edit files in them. Every subfolder is included."
      >
        {dirViews.length > 0
          ? (
            <div class="account-list">
              {dirViews.map((dir) => (
                <div class="account" key={dir.id}>
                  <div>
                    <div class="name">{dir.name}</div>
                    <div class="meta">{dir.meta}</div>
                  </div>
                  <app-button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm("Revoke write access to this directory?")) return;
                      void deleteWritableDir(dir.id)
                        .then(() => listWritableDirs())
                        .then((items) => {
                          dirs = items;
                          loaded = true;
                          error = "";
                        })
                        .catch(() => {
                          error = "Failed to remove directory.";
                        });
                    }}
                  >
                    Remove
                  </app-button>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No writable folders yet. Agents can still write to the thread workspace and project folders." : "Loading folders..."}</p>}
      </app-card>

      <app-card title="Add writable folder" description="Enter a path relative to your files, e.g. Documents or Notes/Personal. The folder and everything under it becomes writable.">
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const form = event.target as HTMLFormElement;
            error = "";
            void submitWritableDir(form)
              .then(() => {
                form.reset();
                return listWritableDirs();
              })
              .then((items) => {
                dirs = items;
                loaded = true;
                error = "";
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to add directory.";
              });
          }}
        >
          <label>Folder path<input name="path" required placeholder="Documents/Notes" autocomplete="off" /></label>
          <div class="actions"><app-button>Add folder</app-button></div>
          <p class="error">{error}</p>
        </form>
      </app-card>
    </>
  );
}
