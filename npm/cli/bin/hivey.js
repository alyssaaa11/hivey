#!/usr/bin/env node
// Runs the hivey binary shipped in this package (native/<os>-<cpu>/). Two commands are
// handled here because an npm install has no checkout to build from: `hivey setup` (the
// install.sh setup, without the build) and `hivey update` (npm install -g @hivey/cli@latest).
"use strict";

const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const PACKAGE = "@hivey/cli";
const root = path.join(__dirname, "..");
const repo = path.join(root, "repo");
const platforms = require(path.join(root, "platforms.json"));

function fail(message) {
  console.error(`hivey: ${message}`);
  process.exit(1);
}

function binary() {
  if (process.env.HIVEY_BINARY_PATH) return process.env.HIVEY_BINARY_PATH;
  const key = `${process.platform}-${process.arch}`;
  const platform = platforms[key];
  if (!platform) {
    fail(`no prebuilt hivey for ${key} (built for: ${Object.keys(platforms).join(", ")})`);
  }
  const exe = path.join(root, "native", key, platform.exe);
  if (!fs.existsSync(exe)) fail(`${exe} is missing: reinstall with npm install -g ${PACKAGE}`);
  if (process.platform !== "win32") {
    try {
      fs.accessSync(exe, fs.constants.X_OK);
    } catch {
      fs.chmodSync(exe, 0o755);
    }
  }
  return exe;
}

const windows = process.platform === "win32";

// Runs a command attached to this terminal; exits with its status. Ctrl-C already reaches the
// command (same terminal), so this launcher only waits for it; kill or a closing terminal is
// passed on instead of orphaning it.
function run(command, args, env, then) {
  const child = spawn(command, args, { stdio: "inherit", env, shell: windows && command.endsWith(".cmd") });
  const forward = (signal) => child.kill(signal);
  const ignore = () => {};
  const signals = windows ? [] : ["SIGTERM", "SIGHUP"];
  process.on("SIGINT", ignore);
  for (const signal of signals) process.on(signal, forward);
  child.on("error", (error) => fail(`cannot run ${command}: ${error.message}`));
  child.on("exit", (code, signal) => {
    process.off("SIGINT", ignore);
    for (const s of signals) process.off(s, forward);
    if (then) then(code ?? 1);
    if (signal) process.kill(process.pid, signal);
    process.exit(code ?? 1);
  });
}

const args = process.argv.slice(2);
const exe = binary();
const env = { ...process.env };
// The binary looks for its plugins, pets and scripts here (a checkout's own HIVEY_REPO wins).
if (!env.HIVEY_REPO) env.HIVEY_REPO = repo;
const fromNpm = env.HIVEY_REPO === repo;

if (args[0] === "setup" && fromNpm) {
  run("bash", [path.join(repo, "install.sh"), "--prebuilt", exe, ...args.slice(1)], env);
} else if (args[0] === "update" && fromNpm) {
  update();
} else {
  run(exe, args, env);
}

// `hivey update` for an npm install; `--check` only compares versions.
function update() {
  const npm = windows ? "npm.cmd" : "npm";
  if (args.includes("--check")) {
    const latest = spawnSync(npm, ["view", PACKAGE, "version"], { encoding: "utf8", shell: windows });
    const installed = require(path.join(root, "package.json")).version;
    const newest = (latest.stdout || "").trim();
    if (!newest) fail(`could not ask npm for the latest ${PACKAGE}`);
    console.log(
      newest === installed
        ? `hivey ${installed} is up to date`
        : `hivey ${newest} is the latest on npm (installed: ${installed}); hivey update installs it`,
    );
    process.exit(0);
  }
  console.log(`hivey was installed with npm: npm install -g ${PACKAGE}@latest`);
  run(npm, ["install", "-g", `${PACKAGE}@latest`], env, (status) => {
    if (status === 0) {
      console.log("updated: running hivey sessions keep the old version until restarted (hivey server stop)");
    }
  });
}
