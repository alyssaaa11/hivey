#!/usr/bin/env node
// Builds @hivey/cli, the one npm package of a hivey release, from prebuilt binaries: the
// `hivey` command (bin/hivey.js), the binary of each platform in npm/platforms.json
// (native/<os>-<cpu>/), and the checkout files the binary uses at run time (plugins, pets,
// skills, install.sh) in repo/. Its os/cpu fields limit installs to those platforms.
//
//   node scripts/npm/build-packages.mjs --binaries <dir> [--version X.Y.Z] [--out target/npm]
//                                       [--allow-missing] [--pack]
//
// <dir>/<os>-<cpu>/ holds each platform's binary (hivey or hivey.exe, and conpty/ on Windows).
// --allow-missing builds only the platforms found (local tests); a release needs all.
// --pack also writes the .tgz npm publishes into <out>/tarballs.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
// Checkout files the binary reads through HIVEY_REPO (`hivey plugin link`, `hivey pet`, the
// team template's new_wiki.py) and `hivey setup` runs (install.sh).
const REPO_PATHS = ["plugins", "pets", "skills", "install.sh", "LICENSE", "NOTICE", "README.md"];

function parseArgs(argv) {
  const options = { out: path.join(ROOT, "target", "npm"), allowMissing: false, pack: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      return argv[++i];
    };
    if (arg === "--binaries") options.binaries = path.resolve(value());
    else if (arg === "--version") options.version = value().replace(/^v/, "");
    else if (arg === "--out") options.out = path.resolve(value());
    else if (arg === "--allow-missing") options.allowMissing = true;
    else if (arg === "--pack") options.pack = true;
    else throw new Error(`unknown option ${arg}`);
  }
  if (!options.binaries) throw new Error("--binaries <dir> is required");
  options.version ??= cargoVersion();
  return options;
}

function cargoVersion() {
  const cargo = fs.readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  const match = cargo.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("no version in Cargo.toml");
  return match[1];
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function copyTracked(relative, destination) {
  // Only files git tracks: no __pycache__, build output or local state.
  const files = execFileSync("git", ["ls-files", "-z", "--", relative], { cwd: ROOT, encoding: "utf8" })
    .split("\0")
    .filter(Boolean);
  if (files.length === 0) throw new Error(`${relative} is not tracked by git`);
  for (const file of files) {
    const target = path.join(destination, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(path.join(ROOT, file), target);
    fs.chmodSync(target, fs.statSync(path.join(ROOT, file)).mode & 0o777);
  }
}

function copyDir(source, destination) {
  fs.cpSync(source, destination, { recursive: true });
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const { version } = options;
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error(`bad version ${version}`);
  const platforms = JSON.parse(fs.readFileSync(path.join(ROOT, "npm", "platforms.json"), "utf8"));
  const cli = JSON.parse(fs.readFileSync(path.join(ROOT, "npm", "cli", "package.json"), "utf8"));

  fs.rmSync(options.out, { recursive: true, force: true });
  fs.mkdirSync(options.out, { recursive: true });

  const main = path.join(options.out, "cli");
  const built = [];
  const missing = [];
  for (const [key, platform] of Object.entries(platforms)) {
    const source = path.join(options.binaries, key);
    if (!fs.existsSync(path.join(source, platform.exe))) {
      missing.push(key);
      continue;
    }
    const target = path.join(main, "native", key);
    copyDir(source, target);
    if (platform.os !== "win32") fs.chmodSync(path.join(target, platform.exe), 0o755);
    built.push(key);
  }
  if (missing.length && !options.allowMissing) {
    throw new Error(`missing binaries for ${missing.join(", ")} in ${options.binaries}`);
  }
  if (built.length === 0) throw new Error(`no binaries found in ${options.binaries}`);

  copyDir(path.join(ROOT, "npm", "cli", "bin"), path.join(main, "bin"));
  fs.chmodSync(path.join(main, "bin", "hivey.js"), 0o755);
  fs.copyFileSync(path.join(ROOT, "npm", "platforms.json"), path.join(main, "platforms.json"));
  fs.copyFileSync(path.join(ROOT, "npm", "cli", "README.md"), path.join(main, "README.md"));
  for (const relative of REPO_PATHS) copyTracked(relative, path.join(main, "repo"));
  writeJson(path.join(main, "package.json"), {
    ...cli,
    version,
    os: [...new Set(built.map((key) => platforms[key].os))],
    cpu: [...new Set(built.map((key) => platforms[key].cpu))],
  });

  const order = [main];
  if (options.pack) {
    const tarballs = path.join(options.out, "tarballs");
    fs.mkdirSync(tarballs);
    const packed = order.map((dir) => {
      const out = execFileSync("npm", ["pack", "--json", "--pack-destination", tarballs], { cwd: dir, encoding: "utf8" });
      // npm 11 prints a list, npm 12 an object keyed by package name.
      const report = JSON.parse(out);
      const entry = Array.isArray(report) ? report[0] : Object.values(report)[0];
      return path.join(tarballs, entry.filename);
    });
    // publish.sh publishes the tarballs listed here, in this order.
    fs.writeFileSync(path.join(tarballs, "ORDER"), `${packed.map((file) => path.basename(file)).join("\n")}\n`);
  }
  console.log(`hivey ${version}: ${path.relative(ROOT, main)} (${built.join(", ")})`);
  if (missing.length) console.log(`not built (no binary): ${missing.join(", ")}`);
}

try {
  main();
} catch (error) {
  console.error(`build-packages: ${error.message}`);
  process.exit(1);
}
