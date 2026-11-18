#!/usr/bin/env python3
"""Reuse a completed GitHub Actions full check only for the same Git tree."""

import json
import os
import re
import subprocess
from time import monotonic


REPOSITORY = "odineska/eska"
REPOSITORY_ID = 1086457908
WORKFLOWS = ".github/workflows/"
FULL_WORKFLOW = "ci-full.yml"


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


def full_revision(run, workflow):
    """Extract the immutable local reusable-workflow revision from Actions provenance."""
    head = run["head_sha"]
    references = [
        item for item in run.get("referenced_workflows", [])
        if item["path"] == f"{REPOSITORY}/{WORKFLOWS}{FULL_WORKFLOW}@{item['sha']}"
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


def proved_run(get, run, workflow, workflow_id, tree, current_run):
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
    reference = full_revision(run, workflow)
    if reference is None:
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
    name = "check / check" if workflow == "ci.yml" else "check / full / check"
    if workflow == "ci.yml" and len(jobs) != 1:
        return False
    if not any(
        job["name"] == name
        and job["run_id"] == run_id
        and job["run_attempt"] == attempt
        and job["head_sha"] == run["head_sha"]
        and (job["status"], job["conclusion"]) == ("completed", "success")
        for job in jobs
    ):
        return False
    latest = get(path)
    return latest["id"] == run_id and latest["run_attempt"] == attempt


def find_proof(get, revision, tree, current_run):
    """Search recent standalone CI and main release-plz runs. A miss requests full CI."""
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
                if run["id"] in seen:
                    continue
                seen.add(run["id"])
                try:
                    if proved_run(get, run, workflow, definition["id"], tree, current_run):
                        return run["id"], run["run_attempt"]
                except (subprocess.CalledProcessError, AttributeError, KeyError, TypeError, ValueError):
                    # A historical run may lack metadata or have an unavailable synthetic commit.
                    continue
    return None


def main():
    result = None
    try:
        if (
            os.environ["GITHUB_REPOSITORY"] == REPOSITORY
            and os.environ["GITHUB_REPOSITORY_ID"] == str(REPOSITORY_ID)
        ):
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, timeout=5).strip()
            tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], text=True, timeout=5).strip()
            if revision == os.environ["GITHUB_SHA"] and git_hash(revision) and git_hash(tree):
                deadline = monotonic() + 20
                result = find_proof(lambda endpoint: api(endpoint, deadline), revision, tree,
                                    int(os.environ["GITHUB_RUN_ID"]))
    except (OSError, subprocess.SubprocessError, AttributeError, KeyError, TypeError, ValueError) as error:
        print(f"Подтверждение CI недоступно ({type(error).__name__}).")

    if result:
        run_id, attempt = result
        print(f"Проверки дерева подтверждены GitHub Actions: {run_id}, попытка {attempt}.")
    else:
        print("Для этого дерева будет выполнена полная проверка.")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"reused={'true' if result else 'false'}\n")


if __name__ == "__main__":
    main()
