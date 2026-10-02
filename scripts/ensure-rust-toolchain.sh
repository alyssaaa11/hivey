# Sourced by install.sh and scripts/sync-herdr.sh (hiver update). Defines ensure_rust_toolchain.
#
# rust-toolchain.toml pins the Rust version, and the first cargo call would otherwise install it
# silently. When something else installs the same toolchain at the same moment (an editor's
# rust-analyzer, another terminal running cargo), the two rustup processes break each other's
# files ("could not rename ... Directory not empty", "detected conflict"). So: wait until no
# other rustup is downloading, check again (the other process usually finished the job), and
# only then install, removing a broken partial install before each retry.

RUST_CHANNEL=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml 2>/dev/null | head -1)
RUSTUP_LOG="${TMPDIR:-/tmp}"
RUSTUP_LOG="${RUSTUP_LOG%/}/hiver-rustup.log"

# RUSTUP_AUTO_INSTALL=0: a check must never start a hidden toolchain download.
rust_toolchain_ok() {
  RUSTUP_AUTO_INSTALL=0 cargo --version >/dev/null 2>&1 && RUSTUP_AUTO_INSTALL=0 rustc --version >/dev/null 2>&1
}

# Seconds since rustup last wrote a download or unpacked file (large when idle).
rustup_quiet_for() {
  python3 - "${RUSTUP_HOME:-$HOME/.rustup}" <<'EOF'
import os, sys, time
newest = 0
for sub in ("downloads", "tmp"):
    for dirpath, _, files in os.walk(os.path.join(sys.argv[1], sub)):
        for name in files + [""]:
            try:
                newest = max(newest, os.stat(os.path.join(dirpath, name)).st_mtime)
            except OSError:
                pass
print(int(time.time() - newest))
EOF
}

wait_for_other_rustup() {
  local waited=0
  while [ "$(rustup_quiet_for)" -lt 20 ] && [ "$waited" -lt 900 ]; do
    [ "$waited" = 0 ] && echo "another rustup is installing Rust (an editor or terminal); waiting for it..."
    sleep 5
    waited=$((waited + 5))
  done
}

ensure_rust_toolchain() {
  rust_toolchain_ok && return 0
  if ! command -v rustup >/dev/null || [ -z "$RUST_CHANNEL" ]; then
    echo "cargo does not run: $(cargo --version 2>&1 | tail -1)" >&2
    return 1
  fi
  echo "installing Rust $RUST_CHANNEL (pinned in rust-toolchain.toml)..."
  local attempt
  for attempt in 1 2 3; do
    wait_for_other_rustup
    rust_toolchain_ok && return 0
    if [ "$attempt" -gt 1 ]; then
      echo "removing the partial Rust $RUST_CHANNEL install and retrying ($attempt/3)..."
      rustup toolchain uninstall "$RUST_CHANNEL" >/dev/null 2>&1 || true
    fi
    if rustup toolchain install "$RUST_CHANNEL" --profile minimal --component clippy,rustfmt \
         >"$RUSTUP_LOG" 2>&1 && rust_toolchain_ok; then
      return 0
    fi
    sleep 3
  done
  tail -5 "$RUSTUP_LOG" >&2
  cat >&2 <<EOF
could not install Rust $RUST_CHANNEL (full log: $RUSTUP_LOG).
  Close editors and terminals that may be running cargo or rust-analyzer, then run:
    rustup toolchain uninstall $RUST_CHANNEL
  and try again.
EOF
  return 1
}
