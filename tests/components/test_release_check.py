"""Release CI reuse is based on GitHub workflow, attempt and Git tree identity."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
import release_check as check


HEAD = "1" * 40
MERGE = "2" * 40
MAIN = "3" * 40
TREE = "4" * 40
OTHER_TREE = "5" * 40
REPO = {"id": check.REPOSITORY_ID}
CI = "ci.yml"
RELEASE = "release-plz.yml"
WORKFLOW_ID = 17
RUN = 23
ATTEMPT = 2
CURRENT = 31


class ReleaseCheckTests(unittest.TestCase):
    """Model actual Actions REST responses without trusting check names or PR HEAD trees."""

    def setUp(self):
        self.request = {
            "number": 42,
            "head": {"sha": HEAD, "ref": "feature", "repo": REPO},
            "base": {"ref": "main", "repo": REPO},
        }
        self.calls = []

    def witness(self, workflow=CI, event="pull_request"):
        proof_sha = MERGE if event == "pull_request" else HEAD
        branch = "main" if workflow == RELEASE else "feature"
        ref = "refs/pull/42/merge" if event == "pull_request" else f"refs/heads/{branch}"
        self.run_data = {
            "id": RUN,
            "run_attempt": ATTEMPT,
            "workflow_id": WORKFLOW_ID,
            "path": f".github/workflows/{workflow}",
            "repository": REPO,
            "head_repository": REPO,
            "head_sha": HEAD,
            "head_commit": {"id": HEAD, "tree_id": OTHER_TREE if event == "pull_request" else TREE},
            "head_branch": branch,
            "event": event,
            "status": "completed",
            "conclusion": "success",
            "pull_requests": [],
            "referenced_workflows": [{
                "path": f"odineska/eska/.github/workflows/ci-full.yml@{proof_sha}",
                "sha": proof_sha,
                "ref": ref,
            }],
        }
        self.commit = {
            "sha": proof_sha,
            "tree": {"sha": TREE},
            "parents": [{"sha": MAIN}, {"sha": HEAD}],
        }
        self.job = {
            "name": "check / check" if workflow == CI else "check / full / check",
            "run_id": RUN,
            "run_attempt": ATTEMPT,
            "head_sha": HEAD,
            "status": "completed",
            "conclusion": "success",
        }
        self.latest = dict(self.run_data)
        self.jobs = [self.job]

    def api(self, endpoint):
        self.calls.append(endpoint)
        return {
            f"git/commits/{self.commit['sha']}": self.commit,
            "pulls/42": self.request,
            f"actions/runs/{RUN}/attempts/{ATTEMPT}/jobs?per_page=100": {"jobs": self.jobs},
            f"actions/runs/{RUN}": self.latest,
        }[endpoint]

    def proved(self, workflow=CI, current=CURRENT):
        return check.proved_run(self.api, self.run_data, workflow, WORKFLOW_ID, TREE, current)

    def test_pr_uses_synthetic_commit_tree_even_if_the_pr_head_differs(self):
        self.witness()
        self.assertTrue(self.proved())
        self.assertEqual(self.calls[0], f"git/commits/{MERGE}")
        self.assertNotIn(f"git/commits/{HEAD}", self.calls)

    def test_pr_head_tree_is_insufficient_for_a_different_merge_tree(self):
        self.witness()
        self.run_data["head_commit"]["tree_id"] = TREE
        self.commit["tree"]["sha"] = OTHER_TREE
        self.assertFalse(self.proved())

    def test_rejects_nonmatching_synthetic_parent_or_pr(self):
        self.witness()
        self.commit["parents"].reverse()
        self.assertFalse(self.proved())
        self.commit["parents"].reverse()
        self.run_data["pull_requests"] = [{"number": 99}]
        self.assertFalse(self.proved())
        self.run_data["pull_requests"] = []
        self.request["base"]["ref"] = "feature"
        self.assertFalse(self.proved())

    def test_foreign_or_modified_workflow_cannot_supply_evidence_by_job_name(self):
        for key, value in (
            ("workflow_id", WORKFLOW_ID + 1),
            ("path", ".github/workflows/fake.yml"),
            ("repository", {"id": 1}),
            ("head_repository", {"id": 1}),
            ("head_repository", None),
            ("referenced_workflows", []),
            ("referenced_workflows", [{"path": f"fork/eska/.github/workflows/ci-full.yml@{MERGE}",
                                       "sha": MERGE, "ref": "refs/pull/42/merge"}]),
            ("referenced_workflows", [{"path": f"odineska/eska/.github/workflows/ci-full.yml@{MERGE}",
                                       "sha": MERGE, "ref": "refs/heads/feature"}]),
        ):
            with self.subTest(key=key, value=value):
                self.witness()
                self.run_data[key] = value
                self.assertFalse(self.proved())
        self.witness()
        self.jobs.append(dict(self.job, name="other / check"))
        self.assertFalse(self.proved())

    def test_bot_dispatch_and_squash_can_share_tree(self):
        self.witness(event="workflow_dispatch")
        self.assertTrue(self.proved())
        self.commit["tree"]["sha"] = OTHER_TREE
        self.assertFalse(self.proved())

    def test_tag_can_use_successful_main_job_before_publication_finishes(self):
        for event in ("push", "workflow_dispatch"):
            with self.subTest(event=event):
                self.witness(RELEASE, event)
                self.run_data.update(status="in_progress", conclusion=None)
                self.assertTrue(self.proved(RELEASE))
                self.assertFalse(self.proved(RELEASE, current=RUN))
                self.job["conclusion"] = "skipped"
                self.assertFalse(self.proved(RELEASE))

    def test_rejects_wrong_attempt_status_and_changed_run(self):
        self.witness()
        self.run_data["conclusion"] = "failure"
        self.assertFalse(self.proved())
        self.run_data["conclusion"] = "success"
        for key, value in (("conclusion", "failure"), ("status", "in_progress"),
                           ("run_id", 99), ("run_attempt", ATTEMPT - 1), ("head_sha", MERGE)):
            with self.subTest(key=key):
                self.job[key], original = value, self.job[key]
                self.assertFalse(self.proved())
                self.job[key] = original
        self.latest["run_attempt"] += 1
        self.assertFalse(self.proved())

    def test_search_uses_main_proof_and_skips_expensive_pr_search(self):
        self.witness(RELEASE, "push")
        path = f"actions/workflows/{RELEASE}"
        responses = {
            path: {"id": WORKFLOW_ID, "path": f".github/workflows/{RELEASE}"},
            f"{path}/runs?head_sha={MAIN}&per_page=25": {"workflow_runs": [self.run_data]},
        }

        def get(endpoint):
            return responses[endpoint] if endpoint in responses else self.api(endpoint)

        self.assertEqual(check.find_proof(get, MAIN, TREE, CURRENT), (RUN, ATTEMPT))

    def test_expired_search_and_unavailable_api_request_full_ci(self):
        with self.assertRaises(TimeoutError):
            check.api("unused", 0)
        with tempfile.TemporaryDirectory(dir=os.environ["ESKA_TEST_ROOT"]) as temporary:
            output = Path(temporary) / "output"
            variables = {
                "GITHUB_REPOSITORY": check.REPOSITORY,
                "GITHUB_REPOSITORY_ID": str(check.REPOSITORY_ID),
                "GITHUB_SHA": MAIN,
                "GITHUB_RUN_ID": str(CURRENT),
                "GITHUB_OUTPUT": str(output),
            }
            with (
                patch.dict(os.environ, variables),
                patch.object(check.subprocess, "check_output", side_effect=[MAIN, TREE]),
                patch.object(check, "api", side_effect=subprocess.TimeoutExpired("gh", 5)),
            ):
                check.main()
            self.assertEqual(output.read_text(encoding="utf-8"), "reused=false\n")


if __name__ == "__main__":
    unittest.main()
