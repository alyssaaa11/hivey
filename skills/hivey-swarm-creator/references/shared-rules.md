<!-- hivey-swarm:shared-rules:start -->
# Work summary

Always end your final response with `DONE: <3 words max>` on its own line (e.g. `DONE: updated
stop hooks`). hivey's Stop hook reads it as the run's summary and speaks it for masters and solo
agents; swarm workers stay silent.

# Messages

Every message wakes someone and costs their turn. Send only what the other agent must act on:
no acks, thanks or "got it"; one message per hand-off; long content goes in a file and you
send the path. Don't reply to FYIs. Use `@all` only when everyone must act. When a notice says
you and a teammate hit the pair limit, or your hourly limit, stop and batch what's left into
one message (or let the task board and files speak). Your swarm's limits: `hivey swarm comms
<swarm>`.

# Docs: Context7 first

For any library, framework, SDK, API, CLI or cloud service, look the docs up with the
`hivey-context7` skill before a web search or your memory.

# Spend log

A script that calls an LLM API directly (not through Claude Code, Codex or Grok CLI) logs each
call with `hivey_spend.record(response, agent="<agent name>")` from
`~/.claude/skills/hivey-agent-map/scripts/hivey_spend.py`, so the agent map counts its cost.

# Safety and secrets

Agents run with permissions pre-approved; hivey's safety hooks (`hivey safety`, on by default)
block destructive commands and secret access. When one blocks you, ask the user (workers ask the
coordinator); never work around it, and never turn safety off. Secrets come only from the user's
secret store at the moment they're used, never in files, prompts, messages or commits.

# Save skills

When you create a new skill with /skill-creator, offer to publish it with skylls so it can be
reused and shared: with the user's yes, `skylls push <skill folder> -m "<what it does>"` (it
scans for secrets and refuses if it finds any: remove them, never `--skip-scan`). Share
(`skylls share <name> @user`) only when asked. Before creating one, check
`skylls --json find <words> --limit 5`: the user or a friend may already have it.

# Write to Obsidian

Style Obsidian notes with callouts:

```
> [!tip] Title
> Content here
```

Types: note (blue), tip (green), warning (orange), danger (red), info (blue), example (purple),
quote (gray).
<!-- hivey-swarm:shared-rules:end -->
