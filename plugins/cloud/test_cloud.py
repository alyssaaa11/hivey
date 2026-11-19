"""Tests for hivey cloud's bundle and the in-cloud bootstrap, with a fake HOME and envsave.

Run: python3 -m unittest plugins/cloud/test_cloud.py   (needs `age`; skipped without it)
"""

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load_cloud(home: Path):
    os.environ["HOME"] = str(home)
    spec = importlib.util.spec_from_file_location("hivey_cloud_under_test", HERE / "cloud.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


@unittest.skipUnless(shutil.which("age") and shutil.which("age-keygen"), "age not installed")
class BundleTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.old_env = dict(os.environ)
        home = self.home = self.tmp / "home"
        claude = home / ".claude"
        (claude / "skills" / "hivey-demo").mkdir(parents=True)
        (claude / "skills" / "hivey-demo" / "SKILL.md").write_text("---\nname: hivey-demo\n---\n")
        (claude / "skills" / "hivey-demo" / "node_modules").mkdir()
        (claude / "skills" / "hivey-demo" / "node_modules" / "x.js").write_text("x")
        (claude / "skills" / "extra").mkdir()
        (claude / "skills" / "extra" / "SKILL.md").write_text("---\nname: extra\n---\n")
        (claude / "skills" / "private").mkdir()
        (claude / "skills" / "private" / "SKILL.md").write_text("private")
        (claude / "commands").mkdir()
        (claude / "commands" / "go.md").write_text("go")
        (claude / "CLAUDE.md").write_text("Always be kind.")
        (claude / "settings.json").write_text(json.dumps({
            "permissions": {"deny": ["Bash(rm -rf /)", "Read(/Users/me/secret)"]},
            "env": {"DISABLE_AUTOUPDATER": "1", "OPENAI_API_KEY": "sk-no"},
            "hooks": {"Stop": []},
        }))
        (home / ".claude.json").write_text(json.dumps({"mcpServers": {
            "stdio-env": {"type": "stdio", "command": "npx", "args": ["-y", "srv"],
                          "env": {"SRV_TOKEN": "tok-123"}},
            "http-auth": {"type": "http", "url": "https://mcp.example.com",
                          "headers": {"Authorization": "Bearer h-456"}},
            "local": {"type": "stdio", "command": "node", "args": [str(home / "x.js")]},
        }}))
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        fake = bin_dir / "envsave"
        fake.write_text("#!/bin/sh\n[ \"$1\" = get ] && [ \"$2\" = es_abc123 ] && echo db-pass && exit 0\nexit 1\n")
        fake.chmod(0o755)
        os.environ["PATH"] = f"{bin_dir}:{os.environ['PATH']}"
        ident = self.tmp / "id"
        subprocess.run(["age-keygen", "-o", str(ident)], check=True, capture_output=True)
        self.secret = next(l for l in ident.read_text().splitlines() if l.startswith("AGE-SECRET"))
        self.recipient = subprocess.run(["age-keygen", "-y", str(ident)], check=True,
                                        capture_output=True, text=True).stdout.strip()
        repo = self.repo = self.tmp / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
        (repo / ".claude" / "skills" / "extra").mkdir(parents=True)
        (repo / ".claude" / "skills" / "extra" / "SKILL.md").write_text("repo's own")
        (repo / "README.md").write_text("hi")
        subprocess.run(["git", "add", "-A"], cwd=repo, check=True)
        subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "i"],
                       cwd=repo, check=True)
        self.cloud = load_cloud(home)
        self.spec = {
            "skills": ["extra", "missing"], "commands": ["go"], "mcp": ["stdio-env", "http-auth", "local"],
            "secrets": [
                {"name": "DB_PASSWORD", "key": "es_abc123", "kind": "encrypted"},
                {"name": "OPENAI_API_KEY", "key": "es_net999", "kind": "network",
                 "host": "api.openai.com"},
            ],
        }

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self.old_env)
        shutil.rmtree(self.tmp, ignore_errors=True)

    def build(self):
        out = self.tmp / "out"
        out.mkdir()
        return out, self.cloud.build_bundle(self.repo, out, self.spec, self.recipient)

    def test_bundle_copies_only_what_is_listed(self):
        out, manifest = self.build()
        self.assertEqual(manifest["skills"], ["hivey-demo"])  # extra: the repo's own wins
        self.assertTrue((out / ".claude/skills/hivey-demo/SKILL.md").exists())
        self.assertFalse((out / ".claude/skills/hivey-demo/node_modules").exists())
        self.assertFalse((out / ".claude/skills/private").exists())
        self.assertTrue((out / ".claude/commands/go.md").exists())
        joined = "\n".join(manifest["warnings"])
        self.assertIn("extra: the repo has its own", joined)
        self.assertIn("missing: not in", joined)
        self.assertIn("MCP local: skipped", joined)
        rules = (out / ".claude/rules/hivey-cloud-global.md").read_text()
        self.assertIn("Always be kind.", rules)

    def test_settings_keep_safe_parts_and_add_the_bootstrap_hook(self):
        out, _ = self.build()
        settings = json.loads((out / ".claude/settings.json").read_text())
        self.assertEqual(settings["permissions"]["deny"], ["Bash(rm -rf /)"])
        self.assertEqual(settings["env"], {"DISABLE_AUTOUPDATER": "1"})
        self.assertNotIn("Stop", settings["hooks"])
        command = settings["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        self.assertIn(".hivey-cloud/bootstrap.sh", command)

    def test_no_secret_value_is_left_in_plain_text(self):
        out, _ = self.build()
        for path in out.rglob("*"):
            if path.is_file():
                text = path.read_text(errors="ignore")
                for value in ("db-pass", "tok-123", "h-456"):
                    self.assertNotIn(value, text, f"{value} found in {path}")
        mcp = json.loads((out / ".mcp.json").read_text())["mcpServers"]
        self.assertEqual(mcp["stdio-env"]["args"][:3], [".hivey-cloud/mcp-run.sh", "stdio-env", "--"])
        self.assertIn("mcp-headers.sh", mcp["http-auth"]["headersHelper"])
        self.assertNotIn("local", mcp)

    def test_bootstrap_decrypts_and_the_shim_answers(self):
        out, _ = self.build()
        cloud_home = self.tmp / "cloudhome"
        bindir = self.tmp / "cloudbin"
        site = self.tmp / "site"
        for d in (cloud_home, bindir, site):
            d.mkdir()
        env_file = self.tmp / "claude_env"
        env = {**os.environ, "HOME": str(cloud_home), "HIVEY_CLOUD_AGE_KEY": self.secret,
               "HIVEY_CLOUD_BIN": str(bindir), "HIVEY_CLOUD_PYSITE": str(site),
               "CLAUDE_ENV_FILE": str(env_file), "CLAUDE_CODE_REMOTE": "true"}
        subprocess.run(["bash", str(out / ".hivey-cloud/bootstrap.sh")], env=env, check=True)
        get = lambda key: subprocess.run([str(bindir / "envsave"), "get", key], env=env,
                                         capture_output=True, text=True).stdout.strip()
        self.assertEqual(get("es_abc123"), "db-pass")
        self.assertEqual(get("DB_PASSWORD"), "db-pass")
        self.assertEqual(get("es_net999"), "hivey-network-secret")
        self.assertIn("hivey-cloud/env", env_file.read_text())
        exported = subprocess.run(["bash", "-c", f". {env_file}; echo $DB_PASSWORD"], env=env,
                                  capture_output=True, text=True).stdout.strip()
        self.assertEqual(exported, "db-pass")
        headers = subprocess.run(["bash", str(out / ".hivey-cloud/mcp-headers.sh"), "http-auth"],
                                 env=env, capture_output=True, text=True).stdout
        self.assertEqual(json.loads(headers), {"Authorization": "Bearer h-456"})
        ran = subprocess.run(["bash", str(out / ".hivey-cloud/mcp-run.sh"), "stdio-env", "--",
                              "sh", "-c", "echo $SRV_TOKEN"], env=env, capture_output=True, text=True)
        self.assertEqual(ran.stdout.strip(), "tok-123")
        mode = (cloud_home / ".hivey-cloud/secrets.json").stat().st_mode & 0o777
        self.assertEqual(mode, 0o600)

    def test_bootstrap_retries_when_the_key_arrives_later(self):
        out, _ = self.build()
        cloud_home = self.tmp / "h2"
        cloud_home.mkdir()
        env = {**os.environ, "HOME": str(cloud_home), "HIVEY_CLOUD_BIN": str(self.tmp),
               "HIVEY_CLOUD_PYSITE": str(self.tmp), "CLAUDE_CODE_REMOTE": "true"}
        env.pop("HIVEY_CLOUD_AGE_KEY", None)
        subprocess.run(["bash", str(out / ".hivey-cloud/bootstrap.sh")], env=env, check=True)
        self.assertFalse((cloud_home / ".hivey-cloud/ready").exists())
        env["HIVEY_CLOUD_AGE_KEY"] = self.secret
        subprocess.run(["bash", str(out / ".hivey-cloud/bootstrap.sh")], env=env, check=True)
        self.assertTrue((cloud_home / ".hivey-cloud/secrets.json").exists())

    def test_bootstrap_does_nothing_outside_a_cloud_session(self):
        out, _ = self.build()
        cloud_home, bindir = self.tmp / "h3", self.tmp / "b3"
        cloud_home.mkdir(); bindir.mkdir()
        env = {**os.environ, "HOME": str(cloud_home), "HIVEY_CLOUD_AGE_KEY": self.secret,
               "HIVEY_CLOUD_BIN": str(bindir), "HIVEY_CLOUD_PYSITE": str(bindir)}
        env.pop("CLAUDE_CODE_REMOTE", None)
        subprocess.run(["bash", str(out / ".hivey-cloud/bootstrap.sh")], env=env, check=True)
        self.assertEqual(list(bindir.iterdir()), [])
        self.assertFalse((cloud_home / ".hivey-cloud").exists())


if __name__ == "__main__":
    unittest.main()
