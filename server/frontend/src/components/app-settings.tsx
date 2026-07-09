import { Component, css } from "@frontiers-labs/argon";
import { settings } from "../stores/settings.js";
import { AppSettingsEmail } from "./app-settings-email.js";
import { AppSettingsFiles } from "./app-settings-files.js";
import { AppSettingsGithub } from "./app-settings-github.js";
import { AppSettingsGoogle } from "./app-settings-google.js";
import { AppSettingsMemory } from "./app-settings-memory.js";
import { AppSettingsMcp } from "./app-settings-mcp.js";
import { AppSettingsModels } from "./app-settings-models.js";
import { AppSettingsSkills } from "./app-settings-skills.js";
import { AppSettingsTelegram } from "./app-settings-telegram.js";
import { AppSettingsThreads } from "./app-settings-threads.js";

const styles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }

  .root {
    box-sizing: border-box;
    min-height: 100%;
    padding: 32px 24px 64px;
  }

  .shell {
    display: flex;
    flex-direction: column;
    gap: 28px;
    margin: 0 auto;
    max-width: 920px;
    width: 100%;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .page-title {
    color: var(--foreground);
    font-size: 26px;
    letter-spacing: -0.02em;
    line-height: 1.2;
  }

  .lead {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.5;
    margin-top: 6px;
  }

  .layout {
    align-items: start;
    display: grid;
    gap: 28px;
    grid-template-columns: 200px minmax(0, 1fr);
  }

  .tabs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    position: sticky;
    top: 0;
  }

  .tab {
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: var(--muted-foreground);
    cursor: pointer;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    padding: 8px 12px;
    text-align: left;
    transition:
      background-color 140ms ease,
      color 140ms ease;
    white-space: nowrap;
  }

  .tab:hover {
    background: var(--accent);
    color: var(--foreground);
  }

  .layout[data-active="connections"] .tab[data-section="connections"],
  .layout[data-active="email"] .tab[data-section="email"],
  .layout[data-active="mcp"] .tab[data-section="mcp"],
  .layout[data-active="files"] .tab[data-section="files"],
  .layout[data-active="memories"] .tab[data-section="memories"],
  .layout[data-active="skills"] .tab[data-section="skills"],
  .layout[data-active="threads"] .tab[data-section="threads"],
  .layout[data-active="models"] .tab[data-section="models"] {
    background: var(--accent);
    color: var(--foreground);
    font-weight: 600;
  }

  .panels {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-width: 0;
  }

  .panel {
    display: none;
    flex-direction: column;
    gap: 20px;
  }

  .layout[data-active="connections"] .panel[data-panel="connections"],
  .layout[data-active="email"] .panel[data-panel="email"],
  .layout[data-active="mcp"] .panel[data-panel="mcp"],
  .layout[data-active="files"] .panel[data-panel="files"],
  .layout[data-active="memories"] .panel[data-panel="memories"],
  .layout[data-active="skills"] .panel[data-panel="skills"],
  .layout[data-active="threads"] .panel[data-panel="threads"],
  .layout[data-active="models"] .panel[data-panel="models"] {
    display: flex;
  }

  @media (max-width: 760px) {
    .root {
      padding: 24px 16px 48px;
    }

    .layout {
      grid-template-columns: 1fr;
      gap: 16px;
    }

    .tabs {
      flex-direction: row;
      overflow-x: auto;
      position: static;
    }

  }
`;

export function AppSettings(): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="root">
        <div class="shell">
          <header>
            <h1 class="page-title">Settings</h1>
            <p class="lead">Manage account integrations S.T.R.I.D.E. uses on your behalf.</p>
          </header>

          <div class="layout" data-active={settings.activeSection}>
            <nav class="tabs" aria-label="Settings sections">
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
      </div>
    </>
  );
}
