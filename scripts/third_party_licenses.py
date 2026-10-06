#!/usr/bin/env python3
"""Writes THIRD_PARTY_LICENSES.md: every third-party component hivey builds in, with its license text.

hivey: Apache-2.0 section 4(a) and the MIT/BSD licenses of our dependencies require shipping their
license texts with any binary distribution. Sources: Rust crates from `cargo metadata`, vendored code
under vendor/, and the Zig packages libghostty-vt fetches into vendor/libghostty-vt/zig-pkg (run a
build first so they exist). Identical license texts are printed once and referenced by number.

Usage: python3 scripts/third_party_licenses.py [--check]
  --check  exit 1 if THIRD_PARTY_LICENSES.md is out of date instead of rewriting it
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_LICENSES.md"
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)([-._].*)?$", re.IGNORECASE)
# Zig packages are fetched under content hashes; name them by a file only that package has.
ZIG_PACKAGES = [
    ("zlib.h", "zlib", "Zlib", "https://zlib.net"),
    ("hwy", "Google Highway", "Apache-2.0 OR BSD-3-Clause", "https://github.com/google/highway"),
    ("sync.txt", "Wuffs", "Apache-2.0 OR MIT", "https://github.com/google/wuffs"),
    ("Aardvark Blue", "iTerm2-Color-Schemes", "MIT", "https://github.com/mbadolato/iTerm2-Color-Schemes"),
    ("1x1#000000.png", "test images", "CC0-1.0", "libghostty-vt build test data"),
]
MIT_TEMPLATE = """MIT License

Copyright (c) {holders}

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""


def license_files(directory):
    if not directory.is_dir():
        return []
    return sorted(p for p in directory.iterdir() if p.is_file() and LICENSE_FILE.match(p.name))


def cargo_components():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    members = set(meta["workspace_members"])
    for pkg in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        if pkg["id"] in members:
            continue
        crate_dir = Path(pkg["manifest_path"]).parent
        files = license_files(crate_dir)
        if pkg.get("license_file"):
            extra = crate_dir / pkg["license_file"]
            if extra.is_file() and extra not in files:
                files.append(extra)
        license = pkg.get("license") or "see license file"
        texts = []
        if not files:
            # The crate ships no license file. MIT (offered by every such dependency today) needs
            # the copyright notice, so print the MIT text with the crate's listed authors.
            if "MIT" not in license:
                sys.exit(f"{pkg['name']} {pkg['version']}: no license file and not MIT ({license})")
            holders = ", ".join(a.split(" <")[0] for a in pkg.get("authors") or []) or f"the {pkg['name']} authors"
            texts.append(MIT_TEMPLATE.format(holders=holders))
            license = f"{license} (used under MIT)" if license != "MIT" else license
        yield {
            "name": pkg["name"],
            "version": pkg["version"],
            "license": license,
            "source": pkg.get("repository") or "crates.io",
            "files": files,
            "texts": texts,
        }


def vendored_components():
    vendor = ROOT / "vendor"
    for directory in sorted(p for p in vendor.iterdir() if p.is_dir()):
        if not license_files(directory):
            continue  # vendor/patches: our patch files, not a component
        yield {
            "name": directory.name,
            "version": "vendored",
            "license": "see license file",
            "source": f"vendor/{directory.name}",
            "files": license_files(directory),
        }
    zig_pkgs = vendor / "libghostty-vt" / "zig-pkg"
    if zig_pkgs.is_dir():
        for directory in sorted(p for p in zig_pkgs.iterdir() if p.is_dir()):
            zon = directory / "build.zig.zon"
            name, license, source = directory.name, "see license file", f"zig-pkg/{directory.name}"
            if zon.is_file():
                found = re.search(r"\.name\s*=\s*\.?\"?@?\"?([\w.-]+)", zon.read_text(errors="replace"))
                if found:
                    name = found.group(1)
            for marker, known_name, known_license, url in ZIG_PACKAGES:
                if (directory / marker).exists():
                    name, license, source = known_name, known_license, url
            if name.startswith("N-V-"):
                sys.exit(f"unknown Zig package {directory.name}: add it to ZIG_PACKAGES")
            yield {
                "name": name,
                "version": "Zig package (libghostty-vt build)",
                "license": license,
                "source": source,
                "files": license_files(directory),
            }


def render(components):
    texts = {}
    lines = [
        "# Third-party licenses",
        "",
        "hivey includes the third-party components listed below. Each is used under the license shown;",
        "full license texts follow the list. hivey itself is based on herdr (Apache-2.0, see `NOTICE`).",
        "",
        "Generated by `scripts/third_party_licenses.py`; do not edit by hand.",
        "",
        "| Component | Version | License | Source | Texts |",
        "|---|---|---|---|---|",
    ]
    for comp in components:
        refs = []
        bodies = [path.read_text(errors="replace").strip() for path in comp["files"]]
        for text in bodies + comp.get("texts", []):
            refs.append(texts.setdefault(text, len(texts) + 1))
        ref_cell = ", ".join(f"[{n}](#license-text-{n})" for n in refs) or f"standard {comp['license']} text"
        lines.append(f"| {comp['name']} | {comp['version']} | {comp['license']} | {comp['source']} | {ref_cell} |")
    lines += ["", "## License texts", ""]
    for text, number in texts.items():
        lines += [f"### License text {number}", f'<a id="license-text-{number}"></a>', "", "```text", text, "```", ""]
    return "\n".join(lines)


def main():
    content = render([*cargo_components(), *vendored_components()])
    if "--check" in sys.argv[1:]:
        if not OUT.is_file() or OUT.read_text() != content:
            sys.exit("THIRD_PARTY_LICENSES.md is out of date: run python3 scripts/third_party_licenses.py")
        return
    OUT.write_text(content)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
