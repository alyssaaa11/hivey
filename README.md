<p align="center">
  <img src="assets/logo.svg" alt="hivey" width="100" />
</p>

# hivey

**hivey** is a terminal workspace for swarms of AI coding agents: one swarm per space, a master
you can always find, and a built-in message bus (delivery when the agent is idle, a queue and
inbox, `@all` / `@role:` / `@masters` addressing, a full log). Agents run side by side in panes,
hivey keeps them alive, wakes them on a schedule, and bridges each swarm to Slack.

**Start here:** [`docs/hivey-guide.md`](docs/hivey-guide.md) · Architecture:
[`docs/diagrams/hivey-architecture.html`](docs/diagrams/hivey-architecture.html) · New swarm flow:
[`docs/diagrams/hivey-new-swarm.html`](docs/diagrams/hivey-new-swarm.html) · Write a provider:
[`docs/hivey-providers.md`](docs/hivey-providers.md) · Agent skill: [`skills/hivey/`](skills/hivey/SKILL.md)
(`hivey skill install`) · Design: [`docs/hivey-design.md`](docs/hivey-design.md).

## install

From source (macOS or Linux; needs Rust, Zig, python3 and Node.js — see the header of `install.sh`):

```bash
git clone git@github.com:jcsancho/hivey.git ~/hivey
cd ~/hivey && ./install.sh
```

then start it where the work lives:

```bash
hivey
```

`⌥Q` (or `ctrl+b` `q`) detaches, `hivey` reattaches. Create a swarm with `hivey swarm new "<task>"`.

## development

```bash
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
python3 scripts/third_party_licenses.py   # regenerate THIRD_PARTY_LICENSES.md after dependency changes
```

If you are an AI agent working in this repository, read [`HIVEY.md`](./HIVEY.md) and
[`AGENTS.md`](./AGENTS.md) first.

## credits and license

hivey is based on [herdr](https://github.com/herdrdev/herdr), Copyright the herdr authors,
licensed under the Apache License 2.0. hivey is not affiliated with or endorsed by the herdr
project; "herdr" is used only to describe where this code comes from. See [`NOTICE`](NOTICE).

hivey is licensed under the [Apache License 2.0](LICENSE). Third-party components and their
licenses are listed in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
