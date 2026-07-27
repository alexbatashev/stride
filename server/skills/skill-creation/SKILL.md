---
name: skill-creation
description: Create and update reusable Agent Skills in the user's global skill library. Use when the user asks to teach the agent a repeatable workflow, capture instructions as a skill, or change an existing user skill.
---

# Create or update a skill

Store each user skill as a bundle at `/home/user/skills/<name>`. This directory
is global across threads and writable. Built-in skills at `/usr/share/skills`
are read-only.

## Workflow

1. Identify concrete requests that should trigger the skill and the reusable
   knowledge, scripts, references, or assets it needs.
2. Choose a short, descriptive name. Use only lowercase ASCII letters, digits,
   and single hyphens. Names must be 1-64 characters and must not start or end
   with a hyphen.
3. Check both `/usr/share/skills/<name>` and `/home/user/skills/<name>`.
   Built-in names are reserved. Do not replace an existing user skill unless
   the user asked to update it.
4. Create `/home/user/skills/<name>/SKILL.md` and only the resource directories
   the skill needs.
5. Call `load_skill` with the new skill name. Fix any validation or rendering
   error before reporting success.

## SKILL.md format

Start the file with YAML frontmatter:

```markdown
---
name: example-skill
description: What the skill does and the specific requests that should trigger it.
---

# Example skill

Follow these steps...
```

The frontmatter `name` must exactly match the directory name. Put all trigger
conditions in `description`, because the catalog exposes the description before
the body is loaded. Optional standard fields are `license`, `compatibility`,
`metadata`, and `allowed-tools`.

## Design

- Keep instructions concise and imperative. Include knowledge the agent cannot
  reliably infer.
- Prefer a workflow when order matters. Use task sections when the skill covers
  independent operations.
- Put long or conditional material in `references/` and link it directly from
  `SKILL.md`.
- Put deterministic, reusable automation in `scripts/`; test representative
  scripts before finishing.
- Put templates and files copied into outputs in `assets/`.
- Use `%SKILL_HOME%` for paths inside the bundle, `%USER_HOME%` for
  `/home/user`, and `%WORKSPACE%` for `/home/agent`.
- Dynamic shell context may use `!` followed immediately by a backtick-wrapped
  command. Use it only when loading the skill must capture current local
  context.
- Do not add README files, changelogs, installation guides, or other auxiliary
  documentation.

## Updating

Read the whole existing bundle first. Preserve useful resources and optional
frontmatter. Make the smallest coherent change, then reload the skill to verify
the live result.
