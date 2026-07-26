import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  getThreadRetention,
  updateThreadRetention,
} from "../api/settings.js";

const styles = css`
  .retention-row {
    align-items: center;
    display: flex;
    gap: 16px;
    justify-content: space-between;
  }

  .retention-info {
    min-width: 0;
  }

  .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
  }

  .desc {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.45;
    margin-top: 3px;
  }

  .retention-days {
    align-items: center;
    color: var(--muted-foreground);
    display: flex;
    flex-wrap: wrap;
    font-size: 14px;
    gap: 8px;
    margin-top: 4px;
  }

  .retention-days input {
    background: var(--background);
    border: 1px solid var(--input);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    font-size: 14px;
    height: 34px;
    outline: none;
    padding: 8px 10px;
    text-align: right;
    width: 76px;
  }

  .retention-days input:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .retention-days.off {
    opacity: 0.5;
  }

  .status-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .actions app-button {
    width: auto;
  }

  .saved {
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

export function AppSettingsThreads(): Component {
  let archiveEnabled = state(true);
  let archiveDays = state(14);
  let removeEnabled = state(true);
  let removeDays = state(90);
  let loaded = state(false);
  let error = state("");
  let saved = state(false);

  onMount(() => {
    void getThreadRetention()
      .then((settings) => {
        archiveEnabled = settings.archive_after_days != null;
        archiveDays = settings.archive_after_days ?? 14;
        removeEnabled = settings.remove_after_days != null;
        removeDays = settings.remove_after_days ?? 90;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load thread settings.";
      });
  });

  return (
    <>
      <style>{styles}</style>
      <app-settings-section
        title="Auto-archive"
        description="Archive threads automatically once they have been inactive for a while. Archived threads leave the sidebar but keep all messages and files, and can be restored anytime."
      >
        <div class="retention-row">
          <div class="retention-info">
            <div class="name">Archive inactive threads</div>
            <div class="desc">Turn this off to keep every thread in the sidebar until you archive it yourself.</div>
          </div>
          <app-switch
            checked={archiveEnabled}
            on:change={(event: Event) => {
              const checked = (event as CustomEvent<{ checked: boolean }>).detail?.checked;
              if (typeof checked !== "boolean") return;
              archiveEnabled = checked;
              saved = false;
            }}
          />
        </div>
        <div class={archiveEnabled ? "retention-days" : "retention-days off"}>
          <span>Archive after</span>
          <input
            name="archive-days"
            type="number"
            min="1"
            max="3650"
            value={String(archiveDays)}
            autocomplete="off"
            onInput={(event: Event) => {
              archiveDays = Number((event.target as HTMLInputElement).value);
              saved = false;
            }}
          />
          <span>days of inactivity</span>
        </div>
      </app-settings-section>

      <app-settings-section
        title="Auto-remove"
        description="Permanently delete an archived thread — including its workspace files and version history — once it has stayed archived long enough. This cannot be undone."
      >
        <div class="retention-row">
          <div class="retention-info">
            <div class="name">Delete old archived threads</div>
            <div class="desc">Turn this off to keep archived threads until you delete them yourself.</div>
          </div>
          <app-switch
            checked={removeEnabled}
            on:change={(event: Event) => {
              const checked = (event as CustomEvent<{ checked: boolean }>).detail?.checked;
              if (typeof checked !== "boolean") return;
              removeEnabled = checked;
              saved = false;
            }}
          />
        </div>
        <div class={removeEnabled ? "retention-days" : "retention-days off"}>
          <span>Delete after</span>
          <input
            name="remove-days"
            type="number"
            min="1"
            max="3650"
            value={String(removeDays)}
            autocomplete="off"
            onInput={(event: Event) => {
              removeDays = Number((event.target as HTMLInputElement).value);
              saved = false;
            }}
          />
          <span>days since archival</span>
        </div>
      </app-settings-section>

      <div class="status-row actions">
        <app-button
          onClick={() => {
            error = "";
            saved = false;
            const archive = Math.min(3650, Math.max(1, Math.round(archiveDays) || 14));
            const remove = Math.min(3650, Math.max(1, Math.round(removeDays) || 90));
            void updateThreadRetention({
              archive_after_days: archiveEnabled ? archive : null,
              remove_after_days: removeEnabled ? remove : null,
            })
              .then((settings) => {
                archiveEnabled = settings.archive_after_days != null;
                archiveDays = settings.archive_after_days ?? archiveDays;
                removeEnabled = settings.remove_after_days != null;
                removeDays = settings.remove_after_days ?? removeDays;
                saved = true;
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to save thread settings.";
              });
          }}
        >
          Save changes
        </app-button>
        {saved ? <span class="saved">Saved.</span> : ""}
        {loaded ? "" : <span class="saved">Loading...</span>}
      </div>
      <p class="error">{error}</p>
    </>
  );
}
