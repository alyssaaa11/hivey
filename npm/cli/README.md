# @hivey/cli

[hivey](https://github.com/alyssaaa11/hivey) is a terminal workspace for swarms of AI coding
agents (Claude Code, Codex, Antigravity/Gemini, Grok), based on
[herdr](https://github.com/herdrdev/herdr).

## Install

```bash
npm install -g @hivey/cli
hivey setup      # once: skills, hooks, bundled plugins, Slack, pet and the hivey agent
hivey            # start
```

Prebuilt for macOS (Apple Silicon and Intel), Linux (x64 and arm64) and Windows (x64). npm
picks the right binary (`@hivey/cli-<os>-<cpu>`) as an optional dependency, so don't install
with `--omit=optional`. `hivey setup` needs python3 and runs on macOS and Linux.

## Update

```bash
hivey update           # npm install -g @hivey/cli@latest
hivey update --check   # only compare versions
```

Running hivey sessions keep the old version until restarted (`hivey server stop`).

## Building from source

To change hivey itself, install from a checkout instead (`hivey update` then pulls and
rebuilds it):

```bash
git clone https://github.com/alyssaaa11/hivey.git ~/hivey
cd ~/hivey && ./install.sh
```
