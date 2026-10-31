"""Full installers replace component versions while keeping one installation owner."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ("eska", "eska-project", "eska-check", "eska-build", "eska-vcs", "eska-fmt")


class TakeoverTests(unittest.TestCase):
    """Run the actual shell and PowerShell takeover hooks against real filesystem failures."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="installer-takeover-", dir=os.environ["ESKA_TEST_ROOT"])
        self.addCleanup(self.directory.cleanup)
        self.base = Path(self.directory.name)

    def fixture(self, runtime, components, *, full=False):
        stand = self.base / (runtime + "-" + str(len(list(self.base.iterdir()))))
        prefix = stand / "installation with spaces"
        payload = stand / "payload"
        (prefix / "bin").mkdir(parents=True)
        (prefix / "components").mkdir()
        payload.mkdir()
        suffix = ".exe" if runtime == "ps1" else ""
        names = [tool + suffix for tool in TOOLS]
        for tool, name in zip(TOOLS, names):
            if full or tool in components:
                (prefix / "bin" / name).write_text(tool + " 0.21.1\n", encoding="utf-8")
            (payload / name).write_text(tool + " 0.22.0\n", encoding="utf-8")
        for tool in components:
            (prefix / f"components/{tool}.json").write_text(
                json.dumps({"method": "component-archive", "package": {"version": "0.21.1", "tool": tool}}),
                encoding="utf-8")
        # These belong to users, other components and the independently versioned analyzer.
        for name in ("components/custom.json", "share/eska/eska-project/NOTICE",
                     "analyzer/bin/bsl-analyzer-app", "analyzer/components/bsl-analyzer-app.json", "notes.txt"):
            path = prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("keep: " + name, encoding="utf-8")
        config = stand / "config/eska"
        if full:
            config.mkdir(parents=True)
            (config / "eska-receipt.json").write_text('{"version":"0.21.1"}', encoding="utf-8")
        receipt = {"version": "0.22.0", "binaries": names, "install_prefix": str(prefix / "bin")}
        (payload / "receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
        guard = (ROOT / f"tools/release-install-guard.{runtime}").read_text(encoding="utf-8")
        if runtime == "sh":
            script = """#!/bin/sh
set -u
err() { printf '%s\\n' "$*" >&2; exit 1; }
say() { printf '%s\\n' "$*"; }
ensure() { "$@" || err "fixture command failed: $*"; }
APP_NAME=eska
APP_VERSION=0.22.0
INSTALL_UPDATER="$TAKEOVER_UPDATER"
RECEIPT_HOME="$TAKEOVER_CONFIG"
"""
            script += guard + """
eska_prepare_full_install "$TAKEOVER_PREFIX/bin" "$TAKEOVER_BINARIES"
for name in $TAKEOVER_BINARIES; do
    ensure cp "$TAKEOVER_PAYLOAD/$name" "$TAKEOVER_PREFIX/bin/$name"
done
if [ "$TAKEOVER_CONFLICT" = 1 ]; then
    ensure rm "$TAKEOVER_PREFIX/bin/eska"
    ensure mkdir "$TAKEOVER_PREFIX/bin/eska"
    exit 1
fi
ensure mkdir -p "$RECEIPT_HOME"
ensure cp "$TAKEOVER_PAYLOAD/receipt.json" "$RECEIPT_HOME/eska-receipt.json"
eska_commit_full_install
"""
            command = ["sh"]
        else:
            script = """$ErrorActionPreference = "Stop"
$InformationPreference = "Continue"
$app_version = "0.22.0"
$install_updater = $env:TAKEOVER_UPDATER -eq "1"
$receipt_home = $env:TAKEOVER_CONFIG
"""
            script += guard + """
$bins = $env:TAKEOVER_BINARIES.Split(" ")
try {
  Start-EskaFullInstall (Join-Path $env:TAKEOVER_PREFIX "bin") $bins
  foreach ($name in $bins) {
    Copy-Item -LiteralPath (Join-Path $env:TAKEOVER_PAYLOAD $name) -Destination (Join-Path $env:TAKEOVER_PREFIX "bin/$name")
  }
  if ($env:TAKEOVER_CONFLICT -eq "1") {
    $path = Join-Path $env:TAKEOVER_PREFIX "bin/eska.exe"
    Remove-Item -LiteralPath $path
    $null = New-Item -Path $path -ItemType Directory
    throw "fixture conflict"
  }
  $null = New-Item -Path $receipt_home -ItemType Directory -Force
  Copy-Item -LiteralPath (Join-Path $env:TAKEOVER_PAYLOAD "receipt.json") -Destination (Join-Path $receipt_home "eska-receipt.json")
  Complete-EskaFullInstall
} finally { Stop-EskaFullInstall }
"""
            command = ["pwsh", "-NoProfile", "-File"]
        driver = stand / ("driver." + runtime)
        driver.write_text(script, encoding="utf-8")
        environment = {**os.environ, "TAKEOVER_PREFIX": str(prefix), "TAKEOVER_PAYLOAD": str(payload),
                       "TAKEOVER_CONFIG": str(config), "TAKEOVER_BINARIES": " ".join(names),
                       "TAKEOVER_UPDATER": "1", "TAKEOVER_CONFLICT": "0"}
        return stand, prefix, payload, environment, [*command, str(driver)]

    def run_installer(self, fixture, **environment):
        stand, _, _, env, command = fixture
        return subprocess.run(command, cwd=stand, env={**env, **environment}, capture_output=True,
                              text=True, encoding="utf-8", timeout=30)

    @staticmethod
    def snapshot(stand):
        return {path.relative_to(stand).as_posix(): path.read_bytes() for directory in ("installation with spaces", "config")
                for path in (stand / directory).rglob("*") if path.is_file()}

    def runtimes(self):
        runtimes = (["sh"] if os.name != "nt" else []) + (["ps1"] if shutil.which("pwsh") else [])
        if not runtimes:
            self.skipTest("Shell or PowerShell is required")
        return runtimes

    def test_components_and_repeat_end_at_one_version(self):
        for runtime in self.runtimes():
            for components in (("eska-project",), ("eska-fmt",), ("eska-project", "eska-fmt"), TOOLS):
                for locale in ("ru_RU.UTF-8", "en_US.UTF-8"):
                    with self.subTest(runtime=runtime, components=components, locale=locale):
                        fixture = self.fixture(runtime, components)
                        stand, prefix, _, _, _ = fixture
                        preserved = {name: data for name, data in self.snapshot(stand).items()
                                     if "/bin/eska" not in name and not any(name.endswith(f"/{tool}.json") for tool in TOOLS)}
                        for attempt in range(2):
                            if attempt == 1:
                                name = "eska-fmt" + (".exe" if runtime == "ps1" else "")
                                (prefix / "bin" / name).write_text("eska-fmt 0.21.1\n", encoding="utf-8")
                            result = self.run_installer(fixture, LC_ALL=locale)
                            self.assertEqual(result.returncode, 0, result)
                            if attempt == 0:
                                self.assertIn("полного комплекта" if locale.startswith("ru") else "complete ESKA bundle",
                                              result.stdout + result.stderr)
                            receipt = json.loads((stand / "config/eska/eska-receipt.json").read_bytes())
                            self.assertEqual(receipt["version"], "0.22.0")
                            for tool, name in zip(TOOLS, receipt["binaries"]):
                                self.assertEqual((prefix / "bin" / name).read_text(), tool + " 0.22.0\n")
                                self.assertFalse((prefix / f"components/{tool}.json").exists())
                            self.assertTrue(all((stand / name).read_bytes() == data for name, data in preserved.items()))
                            self.assertFalse(list(prefix.glob(".eska-full-install.*")))
                            self.assertFalse((prefix / ".eska-components.claim").exists())

    def test_failed_copy_or_receipt_write_restores_old_installation(self):
        for runtime in self.runtimes():
            for full in (False, True):
                for failed in ("eska-fmt", "receipt.json"):
                    with self.subTest(runtime=runtime, full=full, failed=failed):
                        fixture = self.fixture(runtime, ("eska-project", "eska-fmt"), full=full)
                        stand, prefix, payload, _, _ = fixture
                        name = failed + (".exe" if runtime == "ps1" and failed != "receipt.json" else "")
                        (payload / name).unlink()
                        before = self.snapshot(stand)
                        result = self.run_installer(fixture)
                        self.assertNotEqual(result.returncode, 0, result)
                        self.assertEqual(self.snapshot(stand), before, result)
                        self.assertFalse(list(prefix.glob(".eska-full-install.*")))
                        self.assertFalse((prefix / ".eska-components.claim").exists())

    def test_active_claim_and_pending_recovery_preserve_all_files(self):
        for runtime in self.runtimes():
            for marker in (".eska-components.claim", ".eska-components.recovering",
                           ".eska-component-interrupted/journal.json"):
                with self.subTest(runtime=runtime, marker=marker):
                    fixture = self.fixture(runtime, ("eska-project",))
                    stand, prefix, _, _, _ = fixture
                    path = prefix / marker
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text('{"pid":1,"nonce":"other-owner"}', encoding="utf-8")
                    before = self.snapshot(stand)
                    result = self.run_installer(fixture, LC_ALL="en_US.UTF-8")
                    self.assertNotEqual(result.returncode, 0, result)
                    self.assertIn("component-installation-busy", result.stdout + result.stderr)
                    self.assertEqual(self.snapshot(stand), before, result)
                    self.assertFalse(list(prefix.glob(".eska-full-install.*")))

    def test_failed_rollback_retains_backups_and_claim(self):
        for runtime in self.runtimes():
            fixture = self.fixture(runtime, ("eska-project",), full=True)
            stand, prefix, _, _, _ = fixture
            result = self.run_installer(fixture, TAKEOVER_CONFLICT="1")
            self.assertNotEqual(result.returncode, 0, result)
            self.assertIn("component-installation-rollback", result.stdout + result.stderr)
            stages = list(prefix.glob(".eska-full-install.*"))
            self.assertEqual(len(stages), 1, result)
            name = "eska" + (".exe" if runtime == "ps1" else "")
            self.assertEqual((stages[0] / "bin" / name).read_text(), "eska 0.21.1\n")
            self.assertTrue((prefix / "bin" / name).is_dir())
            self.assertEqual(list((prefix / "bin" / name).iterdir()), [])
            self.assertTrue((prefix / ".eska-components.claim").is_file())
            self.assertTrue((prefix / "components/eska-project.json").is_file())

    def test_disabled_full_receipt_cannot_retire_component_receipts(self):
        for runtime in self.runtimes():
            fixture = self.fixture(runtime, ("eska-project",))
            before = self.snapshot(fixture[0])
            result = self.run_installer(fixture, TAKEOVER_UPDATER="0", LC_ALL="ru_RU.UTF-8")
            self.assertNotEqual(result.returncode, 0, result)
            self.assertIn("component-installation-receipt-required", result.stdout + result.stderr)
            self.assertIn("Для перехода", result.stdout + result.stderr)
            self.assertEqual(self.snapshot(fixture[0]), before, result)

    @unittest.skipIf(os.name == "nt", "Creating Windows symlinks requires host-specific privileges")
    def test_linked_component_receipt_does_not_authorize_takeover(self):
        for runtime in self.runtimes():
            fixture = self.fixture(runtime, ("eska-project",))
            stand, prefix, _, _, _ = fixture
            receipt = prefix / "components/eska-project.json"
            external = stand / "external.json"
            receipt.rename(external)
            receipt.symlink_to(external)
            before = self.snapshot(stand)
            result = self.run_installer(fixture)
            self.assertNotEqual(result.returncode, 0, result)
            self.assertIn("component-installation-path", result.stdout + result.stderr)
            self.assertEqual(self.snapshot(stand), before, result)
            self.assertTrue(receipt.is_symlink())
            self.assertEqual(external.read_bytes(), receipt.read_bytes())
            self.assertFalse(list(prefix.glob(".eska-full-install.*")))
