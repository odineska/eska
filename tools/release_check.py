#!/usr/bin/env python3
"""Reuse a completed GitHub Actions full check only for the same Git tree."""

import json
import os
import re
import subprocess
from time import monotonic
from urllib.parse import quote

from release_version_only import git, release_version


REPOSITORY = "odineska/eska"
REPOSITORY_ID = 1086457908
WORKFLOWS = ".github/workflows/"
FULL_WORKFLOW = "ci-full.yml"
PACKAGE_WORKFLOW = "release-package.yml"
BOT_ID = 41898282


def git_hash(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value) is not None


def positive_id(value):
    return type(value) is int and value > 0


def api(endpoint, deadline):
    remaining = deadline - monotonic()
    if remaining <= 0:
        raise TimeoutError("CI evidence search exceeded its budget")
    result = subprocess.run(
        ["gh", "api", f"repos/{REPOSITORY}/{endpoint}"], check=True,
        capture_output=True, timeout=min(5, remaining),
    )
    return json.loads(result.stdout)


def pull_number(reference):
    match = re.fullmatch(r"refs/pull/([1-9][0-9]*)/merge", reference.get("ref") or "")
    return match[1] if match else None


def checked_revision(run, workflow, reusable):
    """Extract the immutable local reusable-workflow revision from Actions provenance."""
    head = run["head_sha"]
    references = [
        item for item in run.get("referenced_workflows", [])
        if item["path"] == f"{REPOSITORY}/{WORKFLOWS}{reusable}@{item['sha']}"
    ]
    if len(references) != 1 or not git_hash(references[0]["sha"]):
        return None
    full = references[0]
    if run["event"] == "pull_request" and workflow == "ci.yml":
        pull = pull_number(full)
        return (full["sha"], pull) if pull else None
    if run["event"] == "workflow_dispatch" and workflow == "ci.yml":
        return (head, None) if full["sha"] == head and full.get("ref") == f"refs/heads/{run['head_branch']}" else None
    if workflow == "release-plz.yml" and run["head_branch"] == "main" and run["event"] in ("push", "workflow_dispatch"):
        return (head, None) if full["sha"] == head and full.get("ref") == "refs/heads/main" else None
    return None


def proved_run(get, run, workflow, workflow_id, tree, current_run, reusable=FULL_WORKFLOW):
    """Tie the successful leaf job and its latest attempt to the checked commit and tree."""
    if (
        run["id"] == current_run
        or run["workflow_id"] != workflow_id
        or run["path"] != WORKFLOWS + workflow
        or run["repository"]["id"] != REPOSITORY_ID
        or (run.get("head_repository") or {}).get("id") != REPOSITORY_ID
        or not positive_id(run["id"])
        or not positive_id(run["run_attempt"])
        or not git_hash(run["head_sha"])
    ):
        return False
    if workflow == "ci.yml" and (run["status"], run["conclusion"]) != ("completed", "success"):
        return False
    reference = checked_revision(run, workflow, reusable)
    if reference is None:
        return False
    if reusable == PACKAGE_WORKFLOW and checked_revision(run, workflow, "release-check.yml") != reference:
        return False
    revision, pull = reference
    if pull is None and (run["head_commit"]["id"], run["head_commit"]["tree_id"]) != (revision, tree):
        return False
    commit = get(f"git/commits/{revision}")
    if commit["sha"] != revision or commit["tree"]["sha"] != tree:
        return False
    if pull:
        linked = run.get("pull_requests") or []
        if linked and not any(item["number"] == int(pull) for item in linked):
            return False
        parents = [parent["sha"] for parent in commit["parents"]]
        if len(parents) != 2 or parents[1] != run["head_sha"]:
            return False
        request = get(f"pulls/{pull}")
        if (
            request["number"] != int(pull)
            or request["base"]["repo"]["id"] != REPOSITORY_ID
            or request["base"]["ref"] != "main"
            or request["head"]["repo"]["id"] != REPOSITORY_ID
            or request["head"]["ref"] != run["head_branch"]
            or request["head"]["sha"] != run["head_sha"]
        ):
            return False
    # The publication job can still be running when it dispatches the binary workflow.
    # A successful leaf is enough, but a skipped or inherited check cannot be a witness.
    run_id, attempt = run["id"], run["run_attempt"]
    path = f"actions/runs/{run_id}"
    jobs = get(f"{path}/attempts/{attempt}/jobs?per_page=100")["jobs"]
    prefix = "release-check" if workflow == "ci.yml" else "check"
    names = {f"{prefix} / package / package"} if reusable == PACKAGE_WORKFLOW else {f"{prefix} / full / check"}
    if workflow == "ci.yml" and reusable == FULL_WORKFLOW:
        names.add("check / check")
    matches = [
        job for job in jobs if job["name"] in names
        and job["run_id"] == run_id
        and job["run_attempt"] == attempt
        and job["head_sha"] == run["head_sha"]
        and (job["status"], job["conclusion"]) == ("completed", "success")
    ]
    if len(matches) != 1:
        return False
    latest = get(path)
    return latest["id"] == run_id and latest["run_attempt"] == attempt


def find_proof(get, revision, tree, current_run, include_package=False, matching_head_only=False):
    """Try matching head trees first, then resolve any remaining synthetic PR commits."""
    kinds = (FULL_WORKFLOW, PACKAGE_WORKFLOW) if include_package else (FULL_WORKFLOW,)

    def proof(run, workflow, workflow_id):
        try:
            for kind in kinds:
                if proved_run(get, run, workflow, workflow_id, tree, current_run, kind):
                    return run["id"], run["run_attempt"]
        except (subprocess.CalledProcessError, AttributeError, KeyError, TypeError, ValueError):
            # A historical run may lack metadata or have an unavailable synthetic commit.
            pass
        return None

    synthetic = []
    for workflow, searches in (
        ("release-plz.yml", (f"head_sha={revision}", "branch=main")),
        ("ci.yml", ("status=success",)),
    ):
        path = f"actions/workflows/{workflow}"
        definition = get(path)
        if definition["path"] != WORKFLOWS + workflow or not positive_id(definition["id"]):
            continue
        seen = set()
        for query in searches:
            for run in get(f"{path}/runs?{query}&per_page=25")["workflow_runs"]:
                if run.get("id") == current_run or run.get("id") in seen:
                    continue
                seen.add(run.get("id"))
                if (run.get("head_commit") or {}).get("tree_id") != tree:
                    if not matching_head_only and workflow == "ci.yml" and run.get("event") == "pull_request":
                        synthetic.append((run, workflow, definition["id"]))
                    continue
                result = proof(run, workflow, definition["id"])
                if result:
                    return result
    for candidate in synthetic:
        result = proof(*candidate)
        if result:
            return result
    return None


def source_allows_version_check(get, revision, ref, event):
    """Require a bot-created PR for explicit runs on the release branch."""
    prefix = "refs/heads/release-plz-"
    if event != "workflow_dispatch" or not ref.startswith(prefix):
        return True
    branch = ref.removeprefix("refs/heads/")
    requests = get(f"pulls?head={quote(f'odineska:{branch}', safe='')}&base=main&state=all")
    return any(
        request["user"]["id"] == BOT_ID
        and request["head"]["repo"]["id"] == REPOSITORY_ID
        and request["head"]["ref"] == branch
        and request["head"]["sha"] == revision
        and request["base"]["repo"]["id"] == REPOSITORY_ID
        and request["base"]["ref"] == "main"
        for request in requests
    )


def check_mode(get, revision, tree, current_run, allow_version, ref, event):
    proof = find_proof(get, revision, tree, current_run, include_package=True, matching_head_only=allow_version)
    if proof:
        return "reuse", proof
    if allow_version and source_allows_version_check(get, revision, ref, event):
        parent = git("rev-parse", "HEAD^1").strip()
        if git_hash(parent):
            version = release_version(parent, revision)
            if version and (not ref.startswith("refs/tags/") or ref == f"refs/tags/v{version}"):
                parent_tree = git("rev-parse", f"{parent}^{{tree}}").strip()
                proof = find_proof(get, parent, parent_tree, current_run)
                if proof:
                    return "package", proof
    if allow_version:
        proof = find_proof(get, revision, tree, current_run, include_package=True)
        if proof:
            return "reuse", proof
    return "full", None


def main():
    mode, proof = "full", None
    try:
        if (
            os.environ["GITHUB_REPOSITORY"] == REPOSITORY
            and os.environ["GITHUB_REPOSITORY_ID"] == str(REPOSITORY_ID)
        ):
            revision = git("rev-parse", "HEAD").strip()
            tree = git("rev-parse", "HEAD^{tree}").strip()
            if revision == os.environ["GITHUB_SHA"] and git_hash(revision) and git_hash(tree):
                deadline = monotonic() + 20
                workflows = {}

                def get(endpoint):
                    if not endpoint.startswith("actions/workflows/"):
                        return api(endpoint, deadline)
                    if endpoint not in workflows:
                        workflows[endpoint] = api(endpoint, deadline)
                    return workflows[endpoint]

                mode, proof = check_mode(
                    get, revision, tree, int(os.environ["GITHUB_RUN_ID"]),
                    os.environ.get("ALLOW_VERSION_CHECK") == "true",
                    os.environ["GITHUB_REF"], os.environ["GITHUB_EVENT_NAME"],
                )
    except (OSError, subprocess.SubprocessError, AttributeError, KeyError, TypeError, ValueError) as error:
        print(f"Подтверждение CI недоступно ({type(error).__name__}).")

    if proof:
        run_id, attempt = proof
        stage = "текущего дерева" if mode == "reuse" else "базового дерева"
        print(f"Проверки {stage} подтверждены GitHub Actions: {run_id}, попытка {attempt}.")
        if mode == "package":
            print("Изменились версия и описание релиза. Будет проверена упаковка Cargo.")
    else:
        print("Для этого дерева будет выполнена полная проверка.")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"mode={mode}\n")


if __name__ == "__main__":
    main()
