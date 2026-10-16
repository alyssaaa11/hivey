<!-- hivey-swarm:shared-rules:start -->
# Work summary

Always end your final response with `DONE: <3 words max>` on its own line (e.g. `DONE: updated
stop hooks`). hivey's Stop hook reads it as the run's summary and speaks it for masters and solo
agents; swarm workers stay silent.

# Docs: Context7 first

For any library, framework, SDK, API, CLI or cloud service, look the docs up with the
`hivey-context7` skill before a web search or your memory.

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
