import { Component, css } from "@frontiers-labs/argon";
import { settings } from "../stores/settings.js";
import { AppSettingsEmail } from "./app-settings-email.js";
import { AppSettingsFiles } from "./app-settings-files.js";
import { AppSettingsGithub } from "./app-settings-github.js";
import { AppSettingsGoogle } from "./app-settings-google.js";
import { AppSettingsMemory } from "./app-settings-memory.js";
import { AppSettingsMcp } from "./app-settings-mcp.js";
import { AppSettingsModels } from "./app-settings-models.js";
import { AppSettingsPersonal } from "./app-settings-personal.js";
import { AppSettingsSkills } from "./app-settings-skills.js";
import { AppSettingsTelegram } from "./app-settings-telegram.js";
import { AppSettingsThreads } from "./app-settings-threads.js";

const styles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  .root {
    height: 100%;
    min-height: 0;
    width: 100%;
  }

  .layout {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  .tabs {
    background: var(--background);
    border-right: 1px solid var(--border);
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    padding: 12px 8px;
  }

  .tab {
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--foreground);
    cursor: pointer;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    min-height: 34px;
    padding: 7px 10px;
    text-align: left;
    transition:
      background-color 140ms ease,
      color 140ms ease;
    white-space: nowrap;
  }

  .tab:hover {
    background: var(--sidebar-accent, var(--accent));
  }

  .tab:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%)); outline: none; }

  .layout[data-active="connections"] .tab[data-section="connections"],
  .layout[data-active="personal"] .tab[data-section="personal"],
  .layout[data-active="email"] .tab[data-section="email"],
  .layout[data-active="mcp"] .tab[data-section="mcp"],
  .layout[data-active="files"] .tab[data-section="files"],
  .layout[data-active="memories"] .tab[data-section="memories"],
  .layout[data-active="skills"] .tab[data-section="skills"],
  .layout[data-active="threads"] .tab[data-section="threads"],
  .layout[data-active="models"] .tab[data-section="models"] {
    background: var(--sidebar-accent, var(--accent));
    color: var(--foreground);
    font-weight: 600;
  }

  .panels {
    background: var(--background);
    box-sizing: border-box;
    min-width: 0;
    overflow-y: auto;
    padding: 24px;
  }

  .panel {
    display: none;
    flex-direction: column;
    gap: 24px;
    margin: 0 auto;
    max-width: 720px;
    width: 100%;
  }

  .notice { background: var(--accent); border: 1px solid var(--border); border-radius: 6px; color: var(--foreground); font-size: 13px; line-height: 1.4; margin: 0 auto 20px; max-width: 720px; padding: 9px 11px; }
  .notice:empty { display: none; }

  .layout[data-active="connections"] .panel[data-panel="connections"],
  .layout[data-active="personal"] .panel[data-panel="personal"],
  .layout[data-active="email"] .panel[data-panel="email"],
  .layout[data-active="mcp"] .panel[data-panel="mcp"],
  .layout[data-active="files"] .panel[data-panel="files"],
  .layout[data-active="memories"] .panel[data-panel="memories"],
  .layout[data-active="skills"] .panel[data-panel="skills"],
  .layout[data-active="threads"] .panel[data-panel="threads"],
  .layout[data-active="models"] .panel[data-panel="models"] {
    display: flex;
  }

  @media (max-width: 767px) {
    .layout {
      grid-template-rows: auto minmax(0, 1fr);
      grid-template-columns: 1fr;
    }

    .tabs {
      background: var(--background);
      border-bottom: 1px solid var(--border);
      border-right: 0;
      flex-direction: row;
      min-height: 48px;
      overflow-x: auto;
      overflow-y: hidden;
      padding: 0 8px;
    }
    .tab { align-self: stretch; border-radius: 0; padding: 0 10px; position: relative; }
    .layout[data-active="connections"] .tab[data-section="connections"]::after,
    .layout[data-active="personal"] .tab[data-section="personal"]::after,
    .layout[data-active="email"] .tab[data-section="email"]::after,
    .layout[data-active="mcp"] .tab[data-section="mcp"]::after,
    .layout[data-active="files"] .tab[data-section="files"]::after,
    .layout[data-active="memories"] .tab[data-section="memories"]::after,
    .layout[data-active="skills"] .tab[data-section="skills"]::after,
    .layout[data-active="threads"] .tab[data-section="threads"]::after,
    .layout[data-active="models"] .tab[data-section="models"]::after { background: var(--foreground); bottom: 0; content: ""; height: 2px; inset-inline: 10px; position: absolute; }
    .panels { padding: 20px 16px 40px; }
  }
`;

export function AppSettings(): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="root">
          <div class="layout" data-active={settings.activeSection}>
            <nav class="tabs" aria-label="Settings sections">
              <button type="button" class="tab" data-section="personal" onClick={() => { settings.activeSection = "personal"; }}>Personal</button>
              <button type="button" class="tab" data-section="connections" onClick={() => { settings.activeSection = "connections"; }}>Connections</button>
              <button type="button" class="tab" data-section="email" onClick={() => { settings.activeSection = "email"; }}>Email</button>
              <button type="button" class="tab" data-section="mcp" onClick={() => { settings.activeSection = "mcp"; }}>MCP servers</button>
              <button type="button" class="tab" data-section="files" onClick={() => { settings.activeSection = "files"; }}>Writable folders</button>
              <button type="button" class="tab" data-section="memories" onClick={() => { settings.activeSection = "memories"; }}>Memories</button>
              <button type="button" class="tab" data-section="skills" onClick={() => { settings.activeSection = "skills"; }}>Skills</button>
              <button type="button" class="tab" data-section="models" onClick={() => { settings.activeSection = "models"; }}>Models</button>
              <button type="button" class="tab" data-section="threads" onClick={() => { settings.activeSection = "threads"; }}>Threads</button>
            </nav>

            <div class="panels">
              <div class="notice" role="status">{settings.notice}</div>
              <section class="panel" data-panel="personal">
                <AppSettingsPersonal />
              </section>
              <section class="panel" data-panel="connections">
                <AppSettingsTelegram />
                <AppSettingsGithub />
                <AppSettingsGoogle />
              </section>

              <section class="panel" data-panel="email">
                <AppSettingsEmail />
              </section>

              <section class="panel" data-panel="mcp">
                <AppSettingsMcp />
              </section>

              <section class="panel" data-panel="files">
                <AppSettingsFiles />
              </section>

              <section class="panel" data-panel="memories">
                <AppSettingsMemory />
              </section>

              <section class="panel" data-panel="skills">
                <AppSettingsSkills />
              </section>

              <section class="panel" data-panel="models">
                <AppSettingsModels />
              </section>

              <section class="panel" data-panel="threads">
                <AppSettingsThreads />
              </section>
            </div>
          </div>
      </div>
    </>
  );
}
