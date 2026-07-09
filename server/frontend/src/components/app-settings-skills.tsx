import { Component, css, onMount, state } from "@frontiers-labs/argon";
import {
  createSkill,
  deleteSkill,
  listSkills,
  updateSkill,
  type Skill,
} from "../api/settings.js";

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function skillView(skill: Skill): { id: string; name: string; meta: string } {
  return {
    id: skill.id,
    name: escapeHtml(skill.title),
    meta: escapeHtml(`${skill.name} - ${skill.description}`),
  };
}

async function submitSkill(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  await createSkill({
    name: String(data.get("name") ?? "").trim(),
    title: String(data.get("title") ?? "").trim(),
    description: String(data.get("description") ?? "").trim(),
    content: String(data.get("content") ?? "").trim(),
  });
}

async function submitSkillEdit(form: HTMLFormElement, id: string): Promise<void> {
  const data = new FormData(form);
  await updateSkill(id, {
    title: String(data.get("title") ?? "").trim(),
    description: String(data.get("description") ?? "").trim(),
    content: String(data.get("content") ?? "").trim(),
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

  .row-actions {
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
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
    width: 100%;
  }

  input {
    height: 36px;
  }

  textarea {
    min-height: 200px;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .skill-content textarea {
    font-family:
      ui-monospace,
      SFMono-Regular,
      Menlo,
      monospace;
  }

  .actions,
  .row-actions {
    display: flex;
    gap: 8px;
  }

  .actions app-button,
  .row-actions app-button {
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

export function AppSettingsSkills(): Component {
  let skills = state([] as Skill[]);
  let loaded = state(false);
  let error = state("");
  let editingId = state("");
  let editingTitle = state("");
  let editingDescription = state("");
  let editingContent = state("");

  onMount(() => {
    void listSkills()
      .then((items) => {
        skills = items;
        loaded = true;
        error = "";
      })
      .catch(() => {
        error = "Failed to load skills.";
      });
  });

  const skillViews = skills.map(skillView);

  return (
    <>
      <style>{styles}</style>
      <app-card
        title="Skills"
        description="Skills are reusable instruction sets your agents load on demand. Built-in skills are always available and are not listed here."
      >
        {skillViews.length > 0
          ? (
            <div class="account-list">
              {skillViews.map((skill) => (
                <div class="account" key={skill.id}>
                  <div>
                    <div class="name">{skill.name}</div>
                    <div class="meta">{skill.meta}</div>
                  </div>
                  <div class="row-actions">
                    <app-button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        const found = skills.find((item) => item.id === skill.id);
                        if (!found) return;
                        error = "";
                        editingId = found.id;
                        editingTitle = found.title;
                        editingDescription = found.description;
                        editingContent = found.content;
                      }}
                    >
                      Edit
                    </app-button>
                    <app-button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        if (!window.confirm("Remove this skill from S.T.R.I.D.E.?")) return;
                        void deleteSkill(skill.id)
                          .then(() => listSkills())
                          .then((items) => {
                            if (editingId === skill.id) {
                              editingId = "";
                              editingTitle = "";
                              editingDescription = "";
                              editingContent = "";
                            }
                            skills = items;
                            loaded = true;
                            error = "";
                          })
                          .catch(() => {
                            error = "Failed to remove skill.";
                          });
                      }}
                    >
                      Remove
                    </app-button>
                  </div>
                </div>
              )).join("")}
            </div>
          )
          : <p class="muted">{loaded ? "No skills yet." : "Loading skills..."}</p>}
      </app-card>

      {editingId
        ? (
          <app-card title="Edit skill" description="Update the title, description, or content. The skill name cannot be changed.">
            <form
              onSubmit={(event: Event) => {
                event.preventDefault();
                const form = event.target as HTMLFormElement;
                error = "";
                void submitSkillEdit(form, editingId)
                  .then(() => listSkills())
                  .then((items) => {
                    editingId = "";
                    editingTitle = "";
                    editingDescription = "";
                    editingContent = "";
                    skills = items;
                    loaded = true;
                    error = "";
                  })
                  .catch((err) => {
                    error = err instanceof Error ? err.message : "Failed to update skill.";
                  });
              }}
            >
              <label>Title<input name="title" required value={editingTitle} autocomplete="off" /></label>
              <label>Description<input name="description" required value={editingDescription} autocomplete="off" /></label>
              <label class="skill-content">Content<textarea name="content" required>{editingContent}</textarea></label>
              <div class="actions">
                <app-button>Save changes</app-button>
                <app-button
                  variant="outline"
                  onClick={() => {
                    error = "";
                    editingId = "";
                    editingTitle = "";
                    editingDescription = "";
                    editingContent = "";
                  }}
                >
                  Cancel
                </app-button>
              </div>
              <p class="error">{error}</p>
            </form>
          </app-card>
        )
        : (
          <app-card title="Add skill" description="The name is a unique slug, e.g. python-debugging. Content is Markdown instructions the agent follows when this skill is active.">
            <form
              onSubmit={(event: Event) => {
                event.preventDefault();
                const form = event.target as HTMLFormElement;
                error = "";
                void submitSkill(form)
                  .then(() => {
                    form.reset();
                    return listSkills();
                  })
                  .then((items) => {
                    skills = items;
                    loaded = true;
                    error = "";
                  })
                  .catch((err) => {
                    error = err instanceof Error ? err.message : "Failed to add skill.";
                  });
              }}
            >
              <label>Name<input name="name" required placeholder="python-debugging" autocomplete="off" pattern="[a-z][a-z0-9-]{1,63}" /></label>
              <label>Title<input name="title" required placeholder="Python Debugging Guide" autocomplete="off" /></label>
              <label>Description<input name="description" required placeholder="One or two sentence summary used for search." autocomplete="off" /></label>
              <label class="skill-content">Content<textarea name="content" required placeholder="Markdown instructions, context, or steps the agent should follow."></textarea></label>
              <div class="actions"><app-button>Add skill</app-button></div>
              <p class="error">{error}</p>
            </form>
          </app-card>
        )}
    </>
  );
}
