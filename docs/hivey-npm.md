# Releasing hivey on npm

hivey is published to npm as one package, `@hivey/cli`:

- `bin/hivey.js`: the `hivey` command, a launcher that runs the binary for this computer
- `native/<os>-<cpu>/hivey`: the prebuilt binary of each platform in `npm/platforms.json`
- `repo/`: the checkout files the binary uses at run time (`plugins/`, `pets/`, `skills/`,
  `install.sh`)

Its `os`/`cpu` fields limit installs to the platforms it ships, so elsewhere npm refuses
with `EBADPLATFORM`.

Releases are built for **Apple Silicon only** (decided 2026-10-23). Version 0.9.35 went out
as `@hivey/cli` plus five platform packages (`@hivey/cli-darwin-arm64`, `-darwin-x64`,
`-linux-arm64`, `-linux-x64`, `-win32-x64`), and all five built fine in CI. Since 0.9.36 the
binary ships inside `@hivey/cli` (user: keep only `@hivey/cli` on npm). To bring other
platforms back, restore their entries in `npm/platforms.json` and in the workflow's build
matrix from commit 7efa5455. Each adds its binary (about 23 MB unpacked) to the package; with
many platforms, per-platform packages (the 0.9.35 layout) would keep downloads smaller.

The launcher sets `HIVEY_REPO` to `@hivey/cli/repo` (unless it's already set), and the binary
finds that folder by itself too (`repo()` in `src/hivey.rs`), so plugins, pets and the team
template work without a checkout. Two commands behave differently in an npm install:

- `hivey setup` runs `repo/install.sh --prebuilt <binary>`: the usual setup (skills, hooks,
  bundled plugins, Slack, pet, the hivey agent) without the build.
- `hivey update` runs `npm install -g @hivey/cli@latest` (`--check` only compares versions).

## Files

- `npm/platforms.json`: the platforms (npm `os`/`cpu`, Rust target, binary name).
- `npm/cli/`: the `@hivey/cli` package (version, `os` and `cpu` are filled in at build time).
- `scripts/npm/build-packages.mjs`: builds the package from binaries, `--pack` writes its tarball.
- `scripts/npm/publish.sh`: publishes the tarballs; skips versions already on npm.
- `scripts/npm/trust.sh`: lets the workflow publish `@hivey/cli` with trusted publishing.
- `.github/workflows/publish-npm.yml`: builds, tests and publishes.

## A release

Nothing to do by hand. Every hivey commit raises the version in `Cargo.toml`, and
`hivey update` pushes `main`. When `main` gets a version that isn't on npm yet, **Publish
npm** builds the binary, packs the package, installs the packed `@hivey/cli` on an Apple
Silicon runner and runs `hivey --version`, then publishes with trusted publishing. A push to
`main` whose version is already on npm does nothing.

A `v*` tag also publishes (the tag must match `Cargo.toml`). Running the workflow by hand
(Actions → Publish npm → Run workflow) only builds and tests, unless "publish" is ticked.
Other branches that change the packaging build and test only. The tarballs are kept as each
run's `npm-packages` artifact.

## First release (once)

Trusted publishing is set per package, and a package must exist before it can trust a
workflow, so the first version (0.9.35, 2026-10-23) was published from a computer:

1. Run **Publish npm** by hand without publishing, then download its `npm-packages` artifact
   and unzip it, e.g. into `~/Downloads/npm-packages`.
2. `npm login` (browser, 2FA; the account needs 2FA enabled to publish).
3. `scripts/npm/publish.sh ~/Downloads/npm-packages` publishes it (2FA).
4. `scripts/npm/trust.sh` connects `@hivey/cli` to `alyssaaa11/hivey`'s `publish-npm.yml`
   (2FA). Check on npmjs.com → the package → Settings → Trusted Publisher.

From then on, new versions on `main` publish with no token. Optionally, under the package's
Settings → Publishing access, require 2FA and disallow tokens, so only the workflow (or you
with 2FA) can publish.

## Testing locally

```bash
cargo build --release
mkdir -p /tmp/hivey-bin/darwin-arm64 && cp target/release/hivey /tmp/hivey-bin/darwin-arm64/
node scripts/npm/build-packages.mjs --binaries /tmp/hivey-bin --pack
npm install --prefix /tmp/hivey-try target/npm/tarballs/hivey-cli-*.tgz
/tmp/hivey-try/node_modules/.bin/hivey --version
```

Installing into a separate prefix leaves the `hivey` on your PATH alone.
