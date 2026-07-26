import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  createProvider,
  createUserModel,
  deleteProvider,
  deleteUserModel,
  getAgentSettings,
  listModels,
  listProviders,
  listUserModels,
  updateAgentSettings,
  type AgentSettings,
  type ModelSummary,
  type ProviderSummary,
  type UserModelSummary,
} from "../api/settings.js";

const DEFAULT_AGENT_SETTINGS: AgentSettings = {
  subagent_allowed_models: [],
  subagent_guidelines: "",
  using_server_defaults: true,
  server_default_guidelines: "",
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

type AccountView = { id: string; name: string; meta: string };

type ModelItemView = { id: string; name: string; meta: string; badge?: string };

function modelSettingsMeta(model: {
  description: string;
  slug: string;
  provider: string;
  vision: boolean;
}): string {
  if (model.description.trim()) {
    return model.description;
  }
  return `${model.slug} - ${model.provider}${model.vision ? " - vision" : ""}`;
}

function configModelView(model: ModelSummary): ModelItemView {
  return {
    id: model.key,
    name: escapeHtml(model.display_name),
    meta: escapeHtml(modelSettingsMeta(model)),
    badge: "Server",
  };
}

function userModelItemView(model: UserModelSummary): AccountView {
  return {
    id: model.id,
    name: escapeHtml(model.display_name),
    meta: escapeHtml(
      modelSettingsMeta({
        description: model.description,
        slug: model.slug,
        provider: model.provider_name,
        vision: model.vision,
      }),
    ),
  };
}

type SubagentModelView = { key: string; label: string; checked: boolean };

function subagentModelView(model: ModelSummary, allowed: string[]): SubagentModelView {
  return {
    key: model.key,
    label: escapeHtml(model.display_name),
    checked: allowed.includes(model.key),
  };
}

async function submitProvider(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  await createProvider({
    name: String(data.get("name") ?? "").trim(),
    kind: String(data.get("kind") ?? "").trim(),
    url: String(data.get("url") ?? "").trim(),
    token: String(data.get("token") ?? "").trim(),
  });
}

async function submitUserModel(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  await createUserModel({
    name: String(data.get("name") ?? "").trim(),
    slug: String(data.get("slug") ?? "").trim(),
    provider_id: String(data.get("provider_id") ?? "").trim(),
    display_name: String(data.get("display_name") ?? "").trim() || null,
    description: String(data.get("description") ?? "").trim() || null,
    reasoning_effort: String(data.get("reasoning_effort") ?? "").trim() || null,
    vision: data.get("vision") === "on",
  });
}

const styles = css`
  .model-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .model-item {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    display: flex;
    gap: 12px;
    justify-content: space-between;
    padding: 12px;
  }

  .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
  }

  .desc,
  .muted {
    color: var(--muted-foreground);
    font-size: 13px;
    line-height: 1.5;
    overflow-wrap: anywhere;
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

  label.full {
    grid-column: 1 / -1;
  }

  input,
  select,
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

  input,
  select {
    height: 36px;
  }

  textarea {
    min-height: 84px;
    resize: vertical;
  }

  .skill-content textarea {
    font-family:
      ui-monospace,
      SFMono-Regular,
      Menlo,
      monospace;
    min-height: 200px;
  }

  input:focus,
  select:focus,
  textarea:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  input::placeholder,
  textarea::placeholder {
    color: var(--muted-foreground);
  }

  .actions app-button,
  .model-item app-button {
    width: auto;
  }

  .checkbox-row {
    align-items: center;
    display: flex;
    gap: 10px;
  }

  .checkbox-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .saved {
    color: var(--muted-foreground);
    font-size: 13px;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
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

export function AppSettingsModels(): Component {
  let availableModels = state([] as ModelSummary[]);
  let providers = state([] as ProviderSummary[]);
  let userModels = state([] as UserModelSummary[]);
  let loaded = state(false);
  let error = state("");
  let agentSettings = state(DEFAULT_AGENT_SETTINGS);
  let agentSettingsError = state("");
  let agentSettingsSaved = state(false);

  onMount(() => {
    void Promise.all([
      listModels(),
      listProviders(),
      listUserModels(),
      getAgentSettings(),
    ])
      .then(([models, providerItems, userModelItems, settings]) => {
        availableModels = models;
        providers = providerItems;
        userModels = userModelItems;
        agentSettings = settings;
        loaded = true;
        error = "";
        agentSettingsError = "";
      })
      .catch(() => {
        error = "Failed to load model settings.";
      });
  });

  const configModelViews = availableModels.filter((model) => model.source === "config").map(configModelView);
  const providerViews = providers.map((provider) => ({
    id: provider.id,
    name: escapeHtml(provider.name),
    meta: escapeHtml(`${provider.kind} - ${provider.url}`),
  }));
  const userModelViews = userModels.map(userModelItemView);
  const subagentModelViews = availableModels.map((model) =>
    subagentModelView(model, agentSettings.subagent_allowed_models),
  );

  return (
    <>
      <style>{styles}</style>
      <app-settings-section title="Server models" description="Add chat models in config.toml under [models.&lt;key&gt;]. Set display_name for labels in the composer and description for this list. Reserved keys embeddings, transcription, title_generator, expert, and explorer are internal and not shown here.">
        {configModelViews.length > 0
          ? (
            <div class="model-list">
              {configModelViews.map((model) => (
                <div class="model-item" key={model.id}>
                  <div>
                    <div class="name">{model.name}</div>
                    <div class="desc">{model.meta}</div>
                  </div>
                  <app-badge variant="secondary">{model.badge}</app-badge>
                </div>
              ))}
            </div>
          )
          : <p class="muted">{loaded ? "No server models are configured." : "Loading models..."}</p>}
        <p class="muted">Example: duplicate a [models.*] block in config.toml.example, set display_name and description, then restart the server.</p>
      </app-settings-section>

      <app-settings-section title="Providers" description="Add your own LLM provider credentials. Models you define below will use these providers.">
        {providerViews.length > 0
          ? (
            <div class="model-list">
              {providerViews.map((provider) => (
                <div class="model-item" key={provider.id}>
                  <div>
                    <div class="name">{provider.name}</div>
                    <div class="desc">{provider.meta}</div>
                  </div>
                  <app-button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm("Remove this provider and its models?")) return;
                      void deleteProvider(provider.id)
                        .then(() => Promise.all([
                          listModels(),
                          listProviders(),
                          listUserModels(),
                          getAgentSettings(),
                        ]))
                        .then(([models, providerItems, userModelItems, settings]) => {
                          availableModels = models;
                          providers = providerItems;
                          userModels = userModelItems;
                          agentSettings = settings;
                          loaded = true;
                          error = "";
                          agentSettingsError = "";
                        })
                        .catch(() => {
                          error = "Failed to remove provider.";
                        });
                    }}
                  >
                    Remove
                  </app-button>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No personal providers yet." : "Loading providers..."}</p>}
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const form = event.target as HTMLFormElement;
            error = "";
            void submitProvider(form)
              .then(() => {
                form.reset();
                return Promise.all([
                  listModels(),
                  listProviders(),
                  listUserModels(),
                  getAgentSettings(),
                ]);
              })
              .then(([models, providerItems, userModelItems, settings]) => {
                availableModels = models;
                providers = providerItems;
                userModels = userModelItems;
                agentSettings = settings;
                loaded = true;
                error = "";
                agentSettingsError = "";
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to add provider.";
              });
          }}
        >
          <div class="grid">
            <label>Name<input name="name" required placeholder="my_openai" autocomplete="off" pattern="[A-Za-z0-9_](?:[A-Za-z0-9_]|-)*" /></label>
            <label>Kind<select name="kind" required>
              <option value="openai">OpenAI</option>
              <option value="openrouter">OpenRouter</option>
              <option value="anthropic">Anthropic</option>
              <option value="ollama">Ollama</option>
              <option value="ollama_cloud">Ollama Cloud</option>
            </select></label>
            <label class="full">URL<input name="url" type="url" required placeholder="https://api.openai.com/v1" autocomplete="off" /></label>
            <label class="full">API token<input name="token" type="password" required autocomplete="off" /></label>
          </div>
          <div class="actions"><app-button type="submit">Add provider</app-button></div>
        </form>
      </app-settings-section>

      <app-settings-section title="Personal models" description="Define models that use your providers. The registry key is internal; display_name is shown in the composer and description appears here.">
        {userModelViews.length > 0
          ? (
            <div class="model-list">
              {userModelViews.map((model) => (
                <div class="model-item" key={model.id}>
                  <div>
                    <div class="name">{model.name}</div>
                    <div class="desc">{model.meta}</div>
                  </div>
                  <app-button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm("Remove this model?")) return;
                      void deleteUserModel(model.id)
                        .then(() => Promise.all([
                          listModels(),
                          listProviders(),
                          listUserModels(),
                          getAgentSettings(),
                        ]))
                        .then(([models, providerItems, userModelItems, settings]) => {
                          availableModels = models;
                          providers = providerItems;
                          userModels = userModelItems;
                          agentSettings = settings;
                          loaded = true;
                          error = "";
                          agentSettingsError = "";
                        })
                        .catch(() => {
                          error = "Failed to remove model.";
                        });
                    }}
                  >
                    Remove
                  </app-button>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No personal models yet." : "Loading models..."}</p>}
        <form
          onSubmit={(event: Event) => {
            event.preventDefault();
            const form = event.target as HTMLFormElement;
            error = "";
            void submitUserModel(form)
              .then(() => {
                form.reset();
                return Promise.all([
                  listModels(),
                  listProviders(),
                  listUserModels(),
                  getAgentSettings(),
                ]);
              })
              .then(([models, providerItems, userModelItems, settings]) => {
                availableModels = models;
                providers = providerItems;
                userModels = userModelItems;
                agentSettings = settings;
                loaded = true;
                error = "";
                agentSettingsError = "";
              })
              .catch((err) => {
                error = err instanceof Error ? err.message : "Failed to add model.";
              });
          }}
        >
          <div class="grid">
            <label>Registry key<input name="name" required placeholder="my_sonnet" autocomplete="off" pattern="[A-Za-z0-9_](?:[A-Za-z0-9_]|-)*" /></label>
            <label>Display name<input name="display_name" placeholder="Claude Sonnet" autocomplete="off" /></label>
            <label class="full">Description<textarea name="description" placeholder="When to use this model." rows="2"></textarea></label>
            <label>Model slug<input name="slug" required placeholder="claude-sonnet-4-20250514" autocomplete="off" /></label>
            <label>Provider<select name="provider_id" required>
              <option value="">Select provider</option>
              {providerViews.map((provider) => <option value={provider.id}>{provider.name}</option>).join("")}
            </select></label>
            <label>Reasoning effort<select name="reasoning_effort">
              <option value="">Disabled</option>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
              <option value="xhigh">XHigh</option>
            </select></label>
            <label class="checkbox-row"><app-checkbox name="vision" value="on" /> Supports vision</label>
          </div>
          <div class="actions"><app-button type="submit">Add model</app-button></div>
        </form>
      </app-settings-section>

      <app-settings-section title="Subagent settings" description="Control which models the main agent can use when spawning subagents, and add guidance on when to pick each one. Users without saved settings inherit the server default routing guide from config.">
        <p class="muted">Allowed subagent models</p>
        <div class="checkbox-list">
          {subagentModelViews.map((model) => (
            <label class="checkbox-row" key={model.key}>
              <app-checkbox
                data-model={model.key}
                checked={model.checked}
                onChange={(event: Event) => {
                  const checked = (event as CustomEvent<{ checked: boolean }>).detail?.checked;
                  if (typeof checked !== "boolean") return;
                  const current = new Set(agentSettings.subagent_allowed_models);
                  if (checked) {
                    current.add(model.key);
                  } else {
                    current.delete(model.key);
                  }
                  agentSettings = {
                    ...agentSettings,
                    subagent_allowed_models: [...current],
                  };
                  agentSettingsSaved = false;
                }}
              />
              <span>{model.label}</span>
            </label>
          )).join("")}
        </div>
        <label class="full skill-content">Model selection guidelines
          <textarea
            name="subagent-guidelines"
            placeholder="Describe when to use faster vs stronger models, cost constraints, or task-specific preferences."
            onInput={(event: Event) => {
              agentSettings = {
                ...agentSettings,
                subagent_guidelines: (event.target as HTMLTextAreaElement).value,
              };
              agentSettingsSaved = false;
            }}
          >{agentSettings.subagent_guidelines}</textarea>
        </label>
        {agentSettings.using_server_defaults
          ? <p class="muted">Showing the server default from config. Save to keep your own copy; your settings will not change when admins update config.</p>
          : <p class="muted">Using your saved settings.</p>}
        <div class="actions">
          <app-button
            onClick={() => {
              agentSettingsError = "";
              agentSettingsSaved = false;
              void updateAgentSettings(agentSettings)
                .then((settings) => {
                  agentSettings = settings;
                  agentSettingsSaved = true;
                })
                .catch((err) => {
                  agentSettingsError =
                    err instanceof Error ? err.message : "Failed to save agent settings.";
                });
            }}
          >
            Save agent settings
          </app-button>
          {agentSettingsSaved ? <span class="saved">Saved.</span> : ""}
        </div>
        <p class="error">{agentSettingsError}</p>
      </app-settings-section>

      <p class="error">{error}</p>
    </>
  );
}
