#!/usr/bin/env python3
"""Exercise final native installers against local artifacts without touching user installation state."""

import argparse
from functools import partial
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

from native_components import metadata_session


def verify_assets(artifacts):
    """Detect upload omissions and altered bytes before invoking any native installer."""
    expected = {}
    for line in (artifacts / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        checksum, name = line.split("  ", 1)
        assert name not in expected, name
        expected[name] = checksum
    files = list(artifacts.iterdir())
    assert all(path.is_file() and not path.is_symlink() for path in files)
    actual = {path.name for path in files} - {"SHA256SUMS"}
    assert set(expected) == actual, {"missing": sorted(set(expected) - actual), "unexpected": sorted(actual - set(expected))}
    for name, checksum in expected.items():
        with (artifacts / name).open("rb") as stream:
            assert hashlib.file_digest(stream, "sha256").hexdigest() == checksum, name
    return len(expected)


def main():
    """Verify clean/repeat installs and the transition from Explorer components to one full version."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", type=Path)
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    verified_assets = verify_assets(artifacts)
    repository = Path(__file__).resolve().parent.parent
    playground = Path(os.environ["ESKA_TEST_ROOT"]).resolve()
    assert playground == (repository.parent / "eska-playground").resolve()
    tools = {"eska", "eska-project", "eska-check", "eska-build", "eska-vcs", "eska-fmt"}
    suffix = ".exe" if os.name == "nt" else ""
    command = (["pwsh", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", artifacts / "eska-installer.ps1"]
               if os.name == "nt" else ["sh", artifacts / "eska-installer.sh"])
    with tempfile.TemporaryDirectory(prefix="release-installer-", dir=playground) as temporary:
        stand = Path(temporary)
        prefix = stand / "installation/bin"
        server = ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=str(artifacts)))
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        temporary_files = stand / "tmp"
        temporary_files.mkdir()
        environment = {key: value for key, value in os.environ.items() if not key.startswith(("ESKA_", "INSTALLER_", "CARGO_DIST_"))}
        environment.update({"ESKA_INSTALL_DIR": str(prefix), "ESKA_NO_MODIFY_PATH": "1",
                            "ESKA_DOWNLOAD_URL": f"http://127.0.0.1:{server.server_port}",
                            "XDG_CONFIG_HOME": str(stand / "config"), "XDG_CACHE_HOME": str(stand / "cache"),
                            "TMPDIR": str(temporary_files), "TMP": str(temporary_files), "TEMP": str(temporary_files)})
        try:
            for _ in range(2):
                result = subprocess.run(command, cwd=stand, env=environment,
                                        capture_output=True, text=True, timeout=120)
                assert result.returncode == 0, result
                assert "no checksums to verify" not in result.stdout + result.stderr
            receipt = json.loads((stand / "config/eska/eska-receipt.json").read_bytes())
            assert set(receipt["binaries"]) == {tool + suffix for tool in tools}, receipt
            assert Path(receipt["install_prefix"]) == prefix, receipt
            version = receipt["version"]
            for tool in sorted(tools):
                binary = prefix / (tool + suffix)
                contract = json.loads(subprocess.check_output([binary, "__tool-contract"], cwd=stand, env=environment, timeout=15))
                assert contract["tool"] == tool and contract["version"] == version, contract
                for locale in ("ru-RU", "en-US"):
                    subprocess.run([binary, "--lang", locale, "--help"], cwd=stand, env=environment,
                                   check=True, capture_output=True, timeout=15)
            for locale in ("ru-RU", "en-US"):
                metadata_session(prefix / ("eska-project" + suffix), locale, stand, environment, [])
            project = prefix / ("eska-project" + suffix)
            target = json.loads(subprocess.check_output([project, "__tool-contract"], timeout=15))["target"]
            index = artifacts / f"release-{target}.json"
            for selected in (("eska-project",), ("eska-fmt",), ("eska-project", "eska-fmt")):
                component_stand = stand / "-".join(selected)
                component_stand.mkdir()
                component_root = component_stand / "installation with spaces"
                component_prefix = component_root / "bin"
                component_environment = {
                    **environment, "ESKA_INSTALL_DIR": str(component_prefix),
                    "XDG_CONFIG_HOME": str(component_stand / "config"),
                    "XDG_CACHE_HOME": str(component_stand / "cache"),
                }
                for tool in selected:
                    subprocess.run([sys.executable, "-B", artifacts / "components.py", "install", "--manifest", index,
                                    "--component", tool, "--root", component_root],
                                   cwd=component_stand, env=component_environment, check=True,
                                   capture_output=True, text=True, timeout=120)
                for name in ("analyzer/bin/bsl-analyzer-app", "analyzer/components/bsl-analyzer-app.json",
                             "components/unrelated.json", "user.txt"):
                    path = component_root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"independent installation")
                preserved = {path: path.read_bytes() for path in component_root.rglob("*")
                             if path.is_file() and ("share" in path.parts or path.name in ("unrelated.json", "user.txt",
                                                                                        "bsl-analyzer-app", "bsl-analyzer-app.json"))}
                for locale in ("ru_RU.UTF-8", "en_US.UTF-8"):
                    result = subprocess.run(command, cwd=component_stand,
                                            env={**component_environment, "LC_ALL": locale},
                                            capture_output=True, text=True, timeout=120)
                    assert result.returncode == 0, result
                    assert "no checksums to verify" not in result.stdout + result.stderr
                    full_receipt = json.loads((component_stand / "config/eska/eska-receipt.json").read_bytes())
                    assert full_receipt["version"] == version, full_receipt
                    assert set(full_receipt["binaries"]) == {tool + suffix for tool in tools}, full_receipt
                    assert Path(full_receipt["install_prefix"]) == component_prefix, full_receipt
                    for tool in tools:
                        binary = component_prefix / (tool + suffix)
                        current = json.loads(subprocess.check_output([binary, "__tool-contract"],
                                                                    cwd=component_stand, env=component_environment, timeout=15))
                        assert current["tool"] == tool and current["version"] == version, current
                        assert not (component_root / f"components/{tool}.json").exists()
                    assert all(path.read_bytes() == data for path, data in preserved.items())
                    assert not (component_root / ".eska-components.claim").exists()
                    assert not list(component_root.glob(".eska-full-install.*"))
                for locale in ("ru-RU", "en-US"):
                    metadata_session(component_prefix / ("eska-project" + suffix), locale, component_stand,
                                     component_environment, [])
                    subprocess.run([component_prefix / ("eska" + suffix), "--lang", locale, "fmt", "--help"],
                                   cwd=component_stand, env=component_environment, check=True,
                                   capture_output=True, timeout=15)
            print(json.dumps({"passed": True, "version": version, "components": sorted(tools),
                              "receipt_isolated": True, "component_ownership_transferred": True,
                              "component_transition_scenarios": 3, "uniform_versions": True,
                              "verified_assets": verified_assets}))
        finally:
            server.shutdown()
            worker.join(timeout=10)
            server.server_close()


if __name__ == "__main__":
    main()
