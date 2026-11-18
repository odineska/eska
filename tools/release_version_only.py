"""Recognize the exact version and changelog patch written by release-plz."""

import re
import subprocess
import tomllib


FILES = {"CHANGELOG.md", "Cargo.lock", "Cargo.toml"}


def git(*arguments):
    return subprocess.check_output(["git", *arguments], timeout=5).decode("utf-8")


def bump_manifest(before, old, new):
    manifest = tomllib.loads(before)
    header = f'[workspace.package]\nversion = "{old}"\n'
    if before.count(header) != 1:
        return None
    updated = before.replace(header, f'[workspace.package]\nversion = "{new}"\n')
    for name, fields in manifest["workspace"].get("dependencies", {}).items():
        path = fields.get("path") if isinstance(fields, dict) else None
        if not path:
            continue
        if fields.get("version") != f"={old}":
            return None
        pin = f'\n{name} = {{ path = "{path}", version = "={old}" }}\n'
        if before.count(pin) != 1:
            return None
        updated = updated.replace(pin, pin.replace(f'"={old}"', f'"={new}"'))
    return updated


def bump_lock(before, old, new, members):
    lock = tomllib.loads(before)
    if set(lock) != {"version", "package"}:
        return None
    packages = lock["package"]
    local = {package["name"] for package in packages if "source" not in package}
    if local != members:
        return None
    updated = before
    for name in sorted(local):
        # Cargo can disambiguate lock entries with the version. If an external package
        # has this same name and version, only full CI can settle its reference changes.
        if sum(package["name"] == name and package["version"] == old for package in packages) != 1:
            return None
        entry = f'[[package]]\nname = "{name}"\nversion = "{old}"\n'
        if before.count(entry) != 1:
            return None
        updated = updated.replace(entry, entry.replace(f'"{old}"', f'"{new}"'))
        before_ref = f'\n "{name} {old}",\n'
        after_ref = f'\n "{name} {new}",\n'
        updated = re.sub(r"(?m)^dependencies = \[\n(?: [^\n]*\n)*\]",
                         lambda match: match[0].replace(before_ref, after_ref), updated)
    return updated


def added_release(before, after, old, new):
    previous = re.search(r"^## \[", before, flags=re.MULTILINE)
    if previous is None or not before[previous.start():].startswith(f"## [{old}]("):
        return False
    prefix, history = before[:previous.start()], before[previous.start():]
    if not after.startswith(prefix) or not after.endswith(history):
        return False
    entry = after[len(prefix):-len(history)]
    compare = f"https://github.com/odineska/eska/compare/v{old}...v{new}"
    heading = re.escape(f"## [{new}]({compare}) - ") + r"[0-9]{4}-[0-9]{2}-[0-9]{2}\n"
    first = re.match(heading, entry)
    return first is not None and re.search(r"^## ", entry[first.end():], re.MULTILINE) is None


def release_version(before, after, read=git):
    """Return the new version only if the Git patch is precisely permitted release metadata."""
    if set(read("diff", "--name-only", "--no-renames", "-z", before, after).strip("\0").split("\0")) != FILES:
        return None

    if read("diff", "--summary", "--no-renames", before, after):
        return None

    def show(commit, path):
        return read("show", f"{commit}:{path}")

    old_manifest, new_manifest = (show(commit, "Cargo.toml") for commit in (before, after))
    old, new = [
        tomllib.loads(contents)["workspace"]["package"]["version"]
        for contents in (old_manifest, new_manifest)
    ]
    version = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    if not all(isinstance(value, str) and re.fullmatch(version, value) for value in (old, new)):
        return None
    if tuple(map(int, old.split("."))) >= tuple(map(int, new.split("."))):
        return None
    if bump_manifest(old_manifest, old, new) != new_manifest:
        return None
    # ESKA's workspace includes the root package and each immediate crate directory.
    manifest = tomllib.loads(old_manifest)
    if manifest["workspace"]["members"] != [".", "crates/*"]:
        return None
    paths = read("ls-tree", "-r", "--name-only", "-z", before, "--", "crates").split("\0")
    members = {manifest["package"]["name"]}
    for path in paths:
        if re.fullmatch(r"crates/[^/]+/Cargo\.toml", path):
            package = tomllib.loads(show(before, path))["package"]
            if package["version"] != {"workspace": True}:
                return None
            members.add(package["name"])
    if bump_lock(show(before, "Cargo.lock"), old, new, members) != show(after, "Cargo.lock"):
        return None
    if not added_release(show(before, "CHANGELOG.md"), show(after, "CHANGELOG.md"), old, new):
        return None
    return new
