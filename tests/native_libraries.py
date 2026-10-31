#!/usr/bin/env python3
"""Validate automatic EPF/ERF library builds with a real platform in an owned playground."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import uuid

from native_build import digest, external_descriptor, interrupt_native, snapshot


def verify_explicit(call, native, binary, root, artifact, managed, cf_file, expected):
    """The same prepared CF must retain the XML result when selected explicitly."""
    label = root.name
    call(f"{label}-explicit-build", [binary, "build", "--base-configuration", cf_file], root)
    target = root.parent / f"{label}-explicit-export"
    call(f"{label}-explicit-export", native + ["infobase", "config", "export", f"--data={managed / 'data'}",
         f"--file={artifact}", str(target)])
    if target.with_suffix(".xml").read_text(encoding="utf-8-sig") != expected:
        raise AssertionError("explicit CF and automatic XML base produced different metadata")


def verify_workspace(call, binary, shell, stand, source, config, version):
    """Exercise inherited settings and two independent application selections natively."""
    other = stand / "second-library"
    shutil.copytree(source, other)
    with (other / "Configuration.xml").open("ab") as stream:
        stream.write(b"\n<!-- second application snapshot -->\n")
    with (config / "config.toml").open("a") as stream:
        stream.write(f"[libraries.second]\nsource={json.dumps(str(other), ensure_ascii=False)}\n")
    root = stand / "workspace"
    root.mkdir()
    for kind in ["processing", "report"]:
        member = root / kind
        shutil.copytree(stand / kind / "src", member / "src")
        (member / "eska.toml").write_text(f"[project]\nname='{kind}'\ntype='{kind}'\n")
    (root / "eska.toml").write_text(
        f"[workspace]\n[build]\nplatform_version='{version}'\n[analysis]\ndefault_application='first'\n"
        "[workspace.members.first]\nconfiguration={library='demo'}\nexternal_objects=['processing']\n"
        "[workspace.members.second]\nconfiguration={library='second'}\nexternal_objects=['report']\n"
    )
    call("workspace-aggregate", [shell, "build", "--workspace", "--manifest", "--format", "json"], root)
    preview = json.loads(call("workspace-preview", [binary, "build", "--workspace", "--dry-run", "--format", "json"], root).stdout)
    snapshots = []
    for project in preview["projects"]:
        artifact = Path(project["artifact"]["path"])
        passport = json.loads(Path(str(artifact) + ".manifest.json").read_text())
        context = passport["source"]["base_context"]
        if context["library"] != project["base_configuration"]["library"]:
            raise AssertionError("a workspace member used another application's library")
        snapshots.append(context["snapshot_id"])
    if len(set(snapshots)) != 2:
        raise AssertionError("independent applications did not capture their own XML source")
    call("workspace-current-member", [shell, "build"], root / "report")
    call("workspace-selector", [binary, "build", "-p", "processing"], root)
    call("workspace-clean", [binary, "clean", "--workspace"], root)
    return {"aggregate": "passed", "member": "passed", "selector": "passed", "independent_bases": True}


def verify_parallel(binary, stand, env):
    """Two real consumers may share preparation, but never the mutable native database."""
    cache = stand / "machine/cache/libraries"
    before = snapshot(cache)
    jobs = []
    started = time.monotonic()
    try:
        for kind, extension in [("processing", "epf"), ("report", "erf")]:
            root = stand / f"parallel-{kind}"
            shutil.copytree(stand / kind / "src", root / "src")
            shutil.copy2(stand / kind / "eska.toml", root / "eska.toml")
            stdout = (stand / f"parallel-{kind}.stdout").open("wb")
            stderr = (stand / f"parallel-{kind}.stderr").open("wb")
            child = subprocess.Popen([str(binary), "build", "--manifest", "--format", "json"],
                                     cwd=root, env=env, stdout=stdout, stderr=stderr)
            jobs.append((root, extension, child, stdout, stderr))
        for root, extension, child, _, _ in jobs:
            if child.wait(timeout=240) != 0:
                raise AssertionError(f"{root.name}: native parallel build failed; see logs")
            artifact = root / "build" / f"{root.name}.{extension}"
            if artifact.stat().st_size == 0:
                raise AssertionError("parallel build published an empty artifact")
            if not (artifact.parent / ".eska/infobases" / artifact.name / "data").is_dir():
                raise AssertionError("parallel consumer has no private native database")
    finally:
        for _, _, child, stdout, stderr in jobs:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=30)
            stdout.close()
            stderr.close()
    if snapshot(cache) != before:
        raise AssertionError("parallel consumers changed the shared prepared result")
    return {"status": "passed", "consumers": 2, "separate_databases": True,
            "shared_cache_unchanged": True, "elapsed_seconds": time.monotonic() - started}


def run(args, repo, stand):
    binary = Path(args.binary or repo / "target/debug/eska-build").resolve()
    shell = Path(args.shell_binary or repo / "target/debug/eska").resolve()
    original = Path(args.library_source).resolve()
    original_bytes = snapshot(original)
    library = stand / "library"
    source = library / "src/cf"
    shutil.copytree(original, source)
    (library / "eska.toml").write_text(
        f"[project]\ntype='configuration'\nsource='src/cf'\n[build]\nplatform_version='{args.platform_version}'\n"
    )
    header = (source / "Configuration.xml").read_text(encoding="utf-8-sig").splitlines()[1]
    config = stand / "machine"
    config.mkdir()
    (config / "config.toml").write_text(
        f"[build]\nrunner='distrobox'\ncontainer={json.dumps(args.distrobox)}\n"
        f"platform_arch={json.dumps(args.platform_arch)}\n[libraries.demo]\nsource={json.dumps(str(source), ensure_ascii=False)}\n"
    )
    env = os.environ.copy()
    for key in ["ESKA_IBCMD", "ESKA_DISTROBOX", "ESKA_PLATFORM_ARCH", "ESKA_LANG"]:
        env.pop(key, None)
    env.update(ESKA_CONFIG_DIR=str(config),
               ESKA_BUILD_BINARY=str(binary), ESKA_VCS_BINARY=str(repo / "target/debug/eska-vcs"))
    native = ["distrobox", "enter", "--name", args.distrobox, "--", args.ibcmd]

    timings = {}

    def call(label, command, cwd=stand, success=True):
        started = time.monotonic()
        result = subprocess.run([str(value) for value in command], cwd=cwd, env=env,
                                capture_output=True, timeout=240)
        timings[label] = time.monotonic() - started
        (stand / f"{label}.stdout").write_bytes(result.stdout)
        (stand / f"{label}.stderr").write_bytes(result.stderr)
        if (result.returncode == 0) != success:
            raise AssertionError(f"{label}: unexpected exit {result.returncode}; see saved logs")
        return result

    call("library-ordinary-build", [shell, "build"], library)
    cache = list((config / "cache/libraries").glob("*/base.json"))
    if len(cache) != 1:
        raise AssertionError("registered library build did not publish one shared CF")
    state_path = cache[0]
    shared_root = state_path.parent
    cf_file = shared_root / "base.cf"
    cf_checksum = digest(cf_file)
    if Path(str(library / "build/library.cf") + ".manifest.json").exists():
        raise AssertionError("ordinary library build unexpectedly required an artifact passport")
    if digest(library / "build/library.cf") != cf_checksum:
        raise AssertionError("shared CF does not match the ordinary library artifact")
    if (shared_root / "template").exists():
        raise AssertionError("a CF-only build eagerly loaded the consumer database template")

    cases = []
    for kind, tag, class_id, extension in [
        ("processing", "ExternalDataProcessor", "c3831ec8-d8d5-4f93-8a22-f9bfae07327f", "epf"),
        ("report", "ExternalReport", "e41aff26-25cf-4bb6-b6c1-3f478a75f374", "erf"),
    ]:
        root = stand / kind
        xml = root / "src"
        xml.mkdir(parents=True)
        name = "EskaLibrary" + kind.title()
        descriptor = external_descriptor(header, name, tag, class_id).decode("utf-8")
        attribute = (f'<Attribute uuid="{uuid.uuid4()}"><Properties><Name>Product</Name>'
                     f'<Synonym/><Comment/><Type><v8:Type>cfg:CatalogRef.{args.catalog}</v8:Type>'
                     '</Type></Properties></Attribute>')
        descriptor = descriptor.replace("<ChildObjects/>", f"<ChildObjects>{attribute}</ChildObjects>")
        (xml / f"{name}.xml").write_bytes(descriptor.encode("utf-8"))
        module = xml / name / "Ext"
        module.mkdir(parents=True)
        (module / "ObjectModule.bsl").write_text(
            f"Функция LibraryProduct() Экспорт\nВозврат Справочники.{args.catalog}.ПустаяСсылка();\nКонецФункции\n"
        )
        manifest = root / "eska.toml"
        manifest.write_text(f"[project]\nname='{kind}'\ntype='{kind}'\n[build]\nplatform_version='{args.platform_version}'\n")
        before = snapshot(xml)
        negative = call(f"{kind}-no-library", [binary, "build", "--format", "json"], root, False)
        if json.loads(negative.stdout)["error"]["stage"] != "import-sources":
            raise AssertionError("negative control did not reach native metadata import")
        manifest.write_text(manifest.read_text() + "[analysis.configuration]\nlibrary='demo'\n")
        previews = [json.loads(call(f"{kind}-preview-{locale}",
            [shell, "--lang", locale, "build", "--dry-run", "--format", "json"], root).stdout)
            for locale in ["ru", "en"]]
        if previews[0] != previews[1]:
            raise AssertionError("dry-run JSON depends on locale")
        plan = previews[0]["projects"][0]
        artifact = Path(plan["artifact"]["path"])
        managed = artifact.parent / ".eska/infobases" / artifact.name
        call(f"{kind}-shell-build", [shell, "build"], root)
        state = json.loads(state_path.read_text())
        cf = digest(cf_file)
        if cf != cf_checksum or (managed / "base.cf").exists():
            raise AssertionError("consumer did not reuse the shared CF")
        templates = list(shared_root.glob("template/state.json"))
        if len(templates) != 1:
            raise AssertionError("consumer did not create one immutable database template")
        template_before = snapshot(templates[0].parent)
        marker = managed / "data/retained"
        marker.write_text("test-owned reuse marker")
        call(f"{kind}-direct-repeat", [binary, "--lang", "en", "build"], root)
        if not marker.is_file() or digest(cf_file) != cf:
            raise AssertionError("repeat did not retain the prepared base and consumer infobase")
        call(f"{kind}-manifest", [shell, "build", "--manifest", "--format", "json"], root)
        passport = Path(str(artifact) + ".manifest.json")
        document = json.loads(passport.read_text())
        if document["source"]["base_context"]["snapshot_id"] != state["source_snapshot"]:
            raise AssertionError("passport does not describe the used source snapshot")
        if document["source"]["base_configuration"]["value"] != cf:
            raise AssertionError("passport does not describe the used CF")
        exported = stand / f"{kind}-export"
        call(f"{kind}-native-export", native + ["infobase", "config", "export", f"--data={managed / 'data'}",
             f"--file={artifact}", str(exported)])
        exported_xml = exported.with_suffix(".xml").read_text(encoding="utf-8-sig")
        if f"cfg:CatalogRef.{args.catalog}" not in exported_xml:
            raise AssertionError("native output lost the library-dependent metadata type")
        verify_explicit(call, native, binary, root, artifact, managed, cf_file, exported_xml)
        call(f"{kind}-recreate", [binary, "build", "--recreate-infobase"], root)
        if marker.exists() or digest(cf_file) != cf:
            raise AssertionError("recreate did not preserve only the verified base CF")
        if snapshot(templates[0].parent) != template_before:
            raise AssertionError("a consumer mutated the shared database template")
        cases.append({"type": kind, "artifact_bytes": artifact.stat().st_size,
                      "negative_without_library": "native_import_failed", "shell_build": "passed",
                      "direct_repeat": "reused_cf_and_infobase", "native_export_type": f"CatalogRef.{args.catalog}",
                      "manifest": "passed", "explicit_cf_matches": True, "recreate": "passed", "source_unchanged": snapshot(xml) == before})
        if snapshot(xml) != before:
            raise AssertionError("native build changed consumer sources")
        print(f"{kind}: passed", flush=True)

    root = stand / "processing"
    artifact = root / "build/processing.epf"
    passport = Path(str(artifact) + ".manifest.json")
    old = (digest(artifact), digest(passport))
    previous = state_path.read_bytes()
    descriptor = source / "Configuration.xml"
    descriptor.write_bytes(descriptor.read_bytes() + b"\n<!-- T147 source refresh -->\n")
    code, stdout = interrupt_native([str(shell), "--project-dir", str(root), "build", "--format", "json"],
        config / "cache/libraries", "*/build.eska-work-*/base.pid", stand, args, env, "cancel-base")
    failure = json.loads(stdout)
    if code != 1 or failure["error"]["stage"] != "prepare-base-configuration":
        raise AssertionError("cancellation did not retain the base-preparation stage")
    if old != (digest(artifact), digest(passport)) or previous != state_path.read_bytes():
        raise AssertionError("cancelled base preparation changed the published result or cache")
    call("refresh-after-cancel", [binary, "build"], root)
    if json.loads(state_path.read_text())["source_snapshot"] == json.loads(previous)["source_snapshot"]:
        raise AssertionError("source bytes changed without refreshing the prepared CF")
    parallel = verify_parallel(binary, stand, env)
    workspace = verify_workspace(call, binary, shell, stand, source, config, args.platform_version)
    for kind, extension in [("processing", "epf"), ("report", "erf")]:
        root = stand / kind
        artifact = root / f"build/{kind}.{extension}"
        checksum = digest(artifact)
        call(f"{kind}-clean", [binary, "clean"], root)
        if digest(artifact) != checksum or (root / f"build/.eska/infobases/{kind}.{extension}").exists():
            raise AssertionError("clean did not remove owned state while preserving the artifact")
        if list((root / "build").glob("build.eska-work-*")):
            raise AssertionError("native operation left staging behind")
    if snapshot(original) != original_bytes:
        raise AssertionError("the original user-provided library was modified")
    return {"complete": True, "platform_version": args.platform_version, "runner": args.distrobox,
            "cases": cases, "cancel_base": "passed", "source_refresh": "passed", "clean": "passed",
            "workspace": workspace, "original_library_unchanged": True,
            "shared_cf_without_manifest": True, "shared_template_immutable": True,
            "parallel_builds": parallel, "timings_seconds": timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library-source", required=True)
    parser.add_argument("--catalog", required=True)
    parser.add_argument("--platform-version", required=True)
    parser.add_argument("--distrobox", required=True)
    parser.add_argument("--platform-arch", required=True)
    parser.add_argument("--ibcmd", required=True)
    parser.add_argument("--binary")
    parser.add_argument("--shell-binary")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    playground = (repo.parent / "eska-playground").resolve()
    if Path(os.environ.get("ESKA_TEST_ROOT", playground)).resolve() != playground:
        parser.error("ESKA_TEST_ROOT must identify the sibling eska-playground")
    stand = Path(tempfile.mkdtemp(prefix="t148-native-", dir=playground))
    print(stand, flush=True)
    (stand / "owner.json").write_text(json.dumps({"kind": "eska-library-build-acceptance", "repository": str(repo)}))
    try:
        result = run(args, repo, stand)
    except Exception as error:
        (stand / "result.json").write_text(json.dumps({"complete": False, "error": str(error)}, indent=2))
        raise
    (stand / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
