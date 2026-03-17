---
name: kanban
description: >
  Turns an approved implementation plan into tracked tickets on the vibe-kanban
  board, spawns isolated agents to implement each one, reviews the results, and
  merges approved work. This is the natural next step after plan mode: once the
  user approves a plan, use this skill to execute it. Each plan step becomes a
  Jira-style "AS IS / TO BE" ticket, assigned to its own agent in an isolated git
  worktree. Also use when the user explicitly says /kanban, mentions the board,
  asks to spawn tasks, orchestrate work, or manage kanban tickets.
disable-model-invocation: false
user-invocable: true
allowed-tools: Bash, Read, Glob, Grep, Agent, Edit, Write
argument-hint: "[coding task or plan to execute]"
---

# Kanban Autonomous Orchestrator

This skill is the execution engine that follows plan mode. The typical flow is:

1. User gives a complex coding task
2. You enter plan mode, explore the codebase, design an approach
3. User approves the plan
4. **This skill takes over** — it turns each plan step into a tracked ticket on
   the vibe-kanban board, spawns isolated agents to implement them in parallel,
   reviews the results, and merges everything together

You can also be invoked directly via `/kanban` for manual board management.

The board gives the user real-time visibility into what's happening, and the
isolated worktrees mean each subtask gets a clean environment without interference.

## Discovering the server

The local vibe-kanban server runs on a dynamic port. Before any API call, find it:

```bash
VK_PORT=$(jq -r '.backend' .dev-ports.json 2>/dev/null)
```

If `.dev-ports.json` doesn't exist, tell the user to start the server with
`pnpm run dev` (or `npx vibe-kanban`).

### Getting an auth token

The local server manages auth sessions. Get a fresh JWT before making remote API calls:

```bash
VK_TOKEN=$(curl -s "http://localhost:${VK_PORT}/api/auth/token" | jq -r '.data.access_token')
```

If this returns null, the user isn't logged in yet — tell them to open
`http://localhost:${VK_PORT}` in a browser and sign in first.

### API base URL

The remote API lives at `https://api.vibekanban.com/v1`. All calls need the Bearer token:

```bash
VK_API="https://api.vibekanban.com/v1"
AUTH=(-H "Authorization: Bearer $VK_TOKEN")
```

### JSON parsing

Always use `jq` for parsing API responses — never Python's `json` module. The API
returns descriptions with literal newlines that `jq` handles natively.

## API reference

### GET organizations
```bash
curl -s "${AUTH[@]}" "$VK_API/organizations" | jq '.organizations[] | {id, name}'
```

### GET projects
```bash
curl -s "${AUTH[@]}" "$VK_API/projects?organization_id=$ORG_ID" | jq '.projects[] | {id, name}'
```

### GET statuses
```bash
curl -s "${AUTH[@]}" "$VK_API/project_statuses?project_id=$PID" | jq '.statuses[]'
```

### GET issues
```bash
curl -s "${AUTH[@]}" "$VK_API/issues?project_id=$PID" | jq '.issues[]'
```

### POST create issue
```bash
curl -s -X POST "${AUTH[@]}" -H "Content-Type: application/json" \
  "$VK_API/issues" \
  -d '{"project_id":"'$PID'","status_id":"'$STATUS_ID'","title":"...","description":"...","sort_order":0,"extension_metadata":{}}' \
  | jq '.issue'
```

### PATCH update issue (transition status)
```bash
curl -s -X PATCH "${AUTH[@]}" -H "Content-Type: application/json" \
  "$VK_API/issues/$ISSUE_ID" \
  -d '{"status_id":"'$TARGET_STATUS_ID'"}' \
  | jq '.issue'
```

### DELETE issue
```bash
curl -s -X DELETE "${AUTH[@]}" "$VK_API/issues/$ISSUE_ID"
```

## Finding project_id

On first use, discover the org and project:

```bash
ORG_ID=$(curl -s "${AUTH[@]}" "$VK_API/organizations" | jq -r '.organizations[0].id')
PID=$(curl -s "${AUTH[@]}" "$VK_API/projects?organization_id=$ORG_ID" | jq -r '.projects[0].id')
```

If there are multiple projects, ask the user which one. Cache the result in
`.kanban-context.json` at the repo root so subsequent invocations are instant:

```bash
echo "{\"project_id\":\"$PID\",\"org_id\":\"$ORG_ID\"}" > .kanban-context.json
```

On subsequent runs, read from cache first:
```bash
PID=$(jq -r '.project_id' .kanban-context.json 2>/dev/null)
```

## Executing an approved plan

When invoked after plan mode (the most common path), `$ARGUMENTS` contains the
task description or the plan itself is in the conversation context. In this case,
skip straight to the orchestrate flow below — don't ask the user to re-describe
the work.

Read the plan from the conversation, then for each step create a ticket using the
AS IS / TO BE format:

```
## AS IS
[Current state — what exists today, reference specific files and functions
from the plan's analysis. Be concrete: paths, line numbers, current behavior.]

## TO BE
[Desired end state after this ticket is done. Include:
- Exact files to create or modify
- New behavior, API surface, or structural changes
- Acceptance criteria the agent should verify before committing]

## Technical notes
[Context from the plan: dependencies on other tickets, patterns to follow,
gotchas discovered during planning. Reference CLAUDE.md conventions.]
```

The plan's structure maps naturally to tickets — each step that touches
independent files or concerns becomes its own ticket. Steps that must be
sequential get a dependency note.

## Commands

Parse `$ARGUMENTS` to decide what to do. If the first word matches a command
below, run that command. Otherwise, treat the entire argument as a task
description and run the `orchestrate` flow.

---

### `board`

Fetch statuses and issues, then render a compact summary grouped by column.
Skip hidden columns unless they have issues.

```bash
curl -s "${AUTH[@]}" "$VK_API/project_statuses?project_id=$PID" > /tmp/vk_statuses.json
curl -s "${AUTH[@]}" "$VK_API/issues?project_id=$PID" > /tmp/vk_issues.json

jq -r --slurpfile issues /tmp/vk_issues.json '
  .project_statuses | sort_by(.sort_order)[] |
  . as $s |
  ($issues[0].issues | map(select(.status_id == $s.id))) as $col |
  if ($col | length) > 0 or ($s.hidden | not) then
    "\($s.name) (\($col | length))" +
    ($col | map("\n  \(.simple_id)  \(.title)") | join(""))
  else empty end' /tmp/vk_statuses.json
```

Example output:
```
To do (3)
  PRJ-12  Fix auth redirect           medium
  PRJ-14  Add rate limiting           high

In progress (1)
  PRJ-11  Refactor DB queries         medium

In review (1)
  PRJ-10  Add user avatars            low

Done (5) — collapsed
```

---

### `create <title> [-- description]`

Create an issue in "To do". If there's a `--` separator, everything after it is the
description. Use the title as a dedupe_key (slugified) so re-running the same create
is safe.

Report the created issue's simple_id.

---

### `transition <simple_id> <status_name>`

Look up the issue by simple_id from the board, then call the transition endpoint.
The status_name is everything after the simple_id (e.g. "In progress", "Done").

Report the transition: `PRJ-5: To do → In progress`

---

### `spawn <simple_id>`

This is the core orchestration primitive. It takes an issue from the board and
assigns an isolated agent to work on it:

1. Fetch the board to find the issue by simple_id — get its title, description, and id
2. Transition the issue to "In progress"
3. Launch an Agent with `isolation: "worktree"` containing a prompt like:

   > You are working on issue {simple_id}: "{title}"
   >
   > {description}
   >
   > Work in this worktree to implement the change. When done, commit your work
   > with a message referencing {simple_id}. Run any relevant checks (cargo check,
   > lint, tests) before committing.

4. Run the agent with `run_in_background: true` so the user isn't blocked
5. Report that the agent has been spawned and the user can check back with
   `/kanban review <simple_id>` when it's done

When the background agent completes, the notification will arrive. At that point,
transition the issue to "In review" and tell the user.

---

### `review <simple_id>`

Review a task that's been worked on by a spawned agent:

1. Find the worktree branch associated with the issue (look in `.claude/worktrees/`
   for a branch whose name or recent commits reference the simple_id)
2. Show a summary of changes: `git diff main...<branch>` stats
3. Run project checks (`cargo check --workspace`, `pnpm run lint`, etc.) on the
   worktree if feasible, or on the diff
4. Report: pass/fail, number of files changed, lines added/removed
5. Suggest next step: `/kanban merge <simple_id>` if it looks good

---

### `merge <simple_id>`

Merge a reviewed task's branch:

1. Verify the issue is in "In review" or "Done"
2. Find the worktree branch
3. Merge it into the current branch: `git merge <branch> --no-ff`
4. Transition the issue to "Done"
5. Clean up the worktree if the merge succeeded
6. Report the result

If there are merge conflicts, report them and don't transition to Done.

---

### `orchestrate <goal>` (or: default when no command matches)

This is the primary flow — especially after an approved plan. If the conversation
already contains an approved plan from plan mode, use it directly. Otherwise,
analyze the goal and decompose it.

#### Step 1 — Create tickets

First, resolve the "To do" status ID:
```bash
TODO_ID=$(curl -s "${AUTH[@]}" "$VK_API/project_statuses?project_id=$PID" \
  | jq -r '.project_statuses[] | select(.name == "To do") | .id')
```

For each subtask, create an issue with the AS IS / TO BE format:

```bash
ISSUE=$(curl -s -X POST "${AUTH[@]}" -H "Content-Type: application/json" \
  "$VK_API/issues" \
  -d '{
    "project_id": "'$PID'",
    "status_id": "'$TODO_ID'",
    "title": "Short imperative title",
    "description": "## AS IS\nCurrent state...\n\n## TO BE\nDesired state...\n\n## Technical notes\n...",
    "sort_order": 0,
    "extension_metadata": {"overseer": {"dedupe_key": "unique-slug"}}
  }')
echo "$ISSUE" | jq '{id: .issue.id, simple_id: .issue.simple_id, title: .issue.title}'
```

Report what was created:
```
Created 3 tickets from plan:
  PRJ-15  Add CSV export button to dashboard component
  PRJ-16  Create CSV generation utility from table data
  PRJ-17  Add E2E test for CSV download flow
```

#### Step 2 — Spawn agents

For each ticket, transition to "In progress" and launch a background agent:

```bash
IN_PROGRESS_ID=$(curl -s "${AUTH[@]}" "$VK_API/project_statuses?project_id=$PID" \
  | jq -r '.project_statuses[] | select(.name == "In progress") | .id')

curl -s -X PATCH "${AUTH[@]}" -H "Content-Type: application/json" \
  "$VK_API/issues/$ISSUE_ID" \
  -d '{"status_id": "'$IN_PROGRESS_ID'"}' | jq '.issue.simple_id'
```

Then spawn via the Agent tool with `isolation: "worktree"` and `run_in_background: true`.
Give each agent a self-contained prompt:

> You are implementing issue {simple_id}: "{title}"
>
> {full AS IS / TO BE description}
>
> This is a Rust + TypeScript project (Cargo workspace, pnpm, Vite, Tailwind).
> Read CLAUDE.md at the repo root for project conventions.
>
> Steps:
> 1. Read the relevant files to understand current state (AS IS)
> 2. Implement the changes described in TO BE
> 3. Run `cargo check --workspace` and `npx pnpm run format`
> 4. Commit with message: "{simple_id}: {title}"
> 5. Do not modify files outside the scope of this ticket.

Launch independent tickets in parallel. If ticket B depends on A, note the
dependency and spawn B only after A completes and is merged.

#### Step 3 — Review completions

As each background agent finishes, transition its ticket to "In review" and:

1. Inspect the diff: `git -C <worktree_path> diff --stat main..HEAD`
2. Run checks: `cargo check --workspace`, `npx pnpm run lint`
3. Verify the TO BE acceptance criteria are met
4. Check the agent stayed in scope (no unrelated file changes)

Report each review:
```
PRJ-15: PASS — 3 files changed, +87 -12, checks clean
PRJ-16: PASS — 1 file created, +45, checks clean
PRJ-17: BLOCKED — depends on PRJ-15, PRJ-16 being merged first
```

#### Step 4 — Merge

Merge approved tickets one at a time:

```bash
git merge <branch> --no-ff -m "Merge {simple_id}: {title}"
```

After each merge, transition to "Done". If a ticket was blocked on a dependency,
spawn a new agent now that the dependency is merged.

If a merge has conflicts, report them and leave the ticket in "In review" for the
user to resolve.

#### Final report

```
Completed: 3/3 tickets merged

  PRJ-15  Add CSV export button           Done
  PRJ-16  Create CSV generation utility    Done
  PRJ-17  Add E2E test for CSV download    Done

All checks passing. 5 files changed, +180 -23.
```

---

## Error handling

- If the server isn't running, say so and suggest `pnpm run dev`
- If a status name doesn't match, list available statuses from the board response
- If an issue simple_id isn't found, show the board so the user can pick the right one
- If a merge has conflicts, report them clearly and leave the issue in "In review"

## When NOT to orchestrate

If the task is a single small change (typo fix, one-liner, config tweak), just do
it directly. The board pipeline adds value when there are genuinely independent
units of work that benefit from parallel execution and isolated review.

Rule of thumb: if it touches one file or one function, implement directly. If the
approved plan has 2+ independent steps touching different concerns, orchestrate.

## Typical end-to-end flow

```
User: "Add dark mode support to the app"
  → Claude enters plan mode, explores codebase, writes plan
  → User approves plan (3 steps: theme context, CSS variables, toggle component)
  → This skill activates:
      Creates PRJ-20, PRJ-21, PRJ-22 on the board (AS IS / TO BE)
      Spawns 3 agents in parallel worktrees
      Reviews each as they complete
      Merges sequentially
      Reports: "3/3 tickets done, dark mode shipped"
```
