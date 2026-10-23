# Releasing hivey on npm

hivey is published to npm as `@hivey/cli` (the `hivey` command) plus one package per platform
with the prebuilt binary, the way esbuild and biome ship native programs:

| Package | Contents |
| --- | --- |
| `@hivey/cli` | `bin/hivey.js` (picks the platform binary), `platforms.json`, and `repo/`: the checkout files the binary uses at run time (`plugins/`, `pets/`, `skills/`, `install.sh`) |
| `@hivey/cli-darwin-arm64` | `bin/hivey` |

npm installs only the platform package matching the computer (`os`/`cpu` in its
`package.json`), as an optional dependency of `@hivey/cli`.

Releases are built for **Apple Silicon only** (decided 2026-10-23). Version 0.9.35 also went
out for `darwin-x64`, `linux-arm64`, `linux-x64` and `win32-x64`, and all five built fine in
CI. To bring them back, restore their entries in `npm/platforms.json` and in the workflow's
build matrix from commit 7efa5455, then run `scripts/npm/trust.sh` for any new packages.

The launcher sets `HIVEY_REPO` to `@hivey/cli/repo` (unless it's already set), and the binary
finds that folder by itself too (`repo()` in `src/hivey.rs`), so plugins, pets and the team
template work without a checkout. Two commands behave differently in an npm install:

- `hivey setup` runs `repo/install.sh --prebuilt <binary>`: the usual setup (skills, hooks,
  bundled plugins, Slack, pet, the hivey agent) without the build.
- `hivey update` runs `npm install -g @hivey/cli@latest` (`--check` only compares versions).

## Files

- `npm/platforms.json`: the platforms (npm `os`/`cpu`, Rust target, binary name).
- `npm/cli/`: the `@hivey/cli` package (version and dependencies are filled in at build time).
- `scripts/npm/build-packages.mjs`: builds all packages from binaries, `--pack` writes tarballs.
- `scripts/npm/publish.sh`: publishes the tarballs, platforms first; skips versions already on npm.
- `scripts/npm/trust.sh`: lets the workflow publish every package with trusted publishing.
- `.github/workflows/publish-npm.yml`: builds, tests and publishes.

## A release

Nothing to do by hand. Every hivey commit raises the version in `Cargo.toml`, and
`hivey update` pushes `main`. When `main` gets a version that isn't on npm yet, **Publish
npm** builds the binaries, packs the packages, installs the packed `@hivey/cli` on an Apple
Silicon runner and runs `hivey --version`, then publishes with trusted publishing. A push to
`main` whose version is already on npm does nothing.

A `v*` tag also publishes (the tag must match `Cargo.toml`). Running the workflow by hand
(Actions → Publish npm → Run workflow) only builds and tests, unless "publish" is ticked.
Other branches that change the packaging build and test only. The tarballs are kept as each
run's `npm-packages` artifact.

## First release (once)

Trusted publishing is set per package, and a package must exist before it can trust a
workflow, so the first version is published from a computer:

1. Run **Publish npm** by hand without publishing, then download its `npm-packages` artifact
   and unzip it, e.g. into `~/Downloads/npm-packages`.
2. `npm login` (browser, 2FA).
3. `scripts/npm/publish.sh ~/Downloads/npm-packages` publishes the packages (2FA each).
4. `scripts/npm/trust.sh` connects each package to `alyssaaa11/hivey`'s `publish-npm.yml`
   (2FA each). Check one on npmjs.com → the package → Settings → Trusted Publisher.

From then on, tags publish with no token. Optionally, under each package's Settings →
Publishing access, require 2FA and disallow tokens, so only the workflow (or you with 2FA)
can publish.

## Testing locally

```bash
cargo build --release
mkdir -p /tmp/hivey-bin/darwin-arm64 && cp target/release/hivey /tmp/hivey-bin/darwin-arm64/
node scripts/npm/build-packages.mjs --binaries /tmp/hivey-bin --allow-missing --pack
npm install -g target/npm/tarballs/hivey-cli-darwin-arm64-*.tgz target/npm/tarballs/hivey-cli-[0-9]*.tgz
hivey --version
```

That replaces any `hivey` already on your PATH from npm; `npm uninstall -g @hivey/cli
@hivey/cli-darwin-arm64` removes it again.
