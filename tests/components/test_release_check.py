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
        self.jobs.append(dict(self.job))
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

    def test_version_gate_prioritizes_baseline_without_resolving_old_prs(self):
        self.witness()
        self.commit["tree"]["sha"] = OTHER_TREE
        old_runs = [
            dict(self.run_data, id=100 + index, head_commit={"tree_id": MAIN})
            for index in range(24)
        ]
        release_path = f"actions/workflows/{RELEASE}"
        ci_path = f"actions/workflows/{CI}"
        responses = {
            release_path: {"id": WORKFLOW_ID, "path": f".github/workflows/{RELEASE}"},
            ci_path: {"id": WORKFLOW_ID, "path": f".github/workflows/{CI}"},
            f"{release_path}/runs?head_sha={MAIN}&per_page=25": {"workflow_runs": []},
            f"{release_path}/runs?head_sha={HEAD}&per_page=25": {"workflow_runs": []},
            f"{release_path}/runs?branch=main&per_page=25": {"workflow_runs": []},
            f"{ci_path}/runs?status=success&per_page=25": {
                "workflow_runs": [*old_runs, self.run_data]
            },
        }

        def get(endpoint):
            return responses[endpoint] if endpoint in responses else self.api(endpoint)

        with (
            patch.object(check, "git", side_effect=[HEAD, OTHER_TREE]),
            patch.object(check, "release_version", return_value="0.35.1"),
        ):
            self.assertEqual(
                check.check_mode(get, MAIN, TREE, CURRENT, True, "refs/heads/main", "push"),
                ("package", (RUN, ATTEMPT)),
            )
        self.assertEqual(self.calls.count(f"git/commits/{MERGE}"), 1)

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
                "GITHUB_REF": "refs/heads/main",
                "GITHUB_EVENT_NAME": "push",
                "GITHUB_OUTPUT": str(output),
            }
            with (
                patch.dict(os.environ, variables),
                patch.object(check, "git", side_effect=[MAIN, TREE]),
                patch.object(check, "api", side_effect=subprocess.TimeoutExpired("gh", 5)),
            ):
                check.main()
            self.assertEqual(output.read_text(encoding="utf-8"), "mode=full\n")


    def test_verified_package_is_reusable_only_for_the_exact_release_tree(self):
        self.witness(event="workflow_dispatch")
        reference = self.run_data["referenced_workflows"][0]
        self.run_data["referenced_workflows"] = [
            dict(reference, path=reference["path"].replace("ci-full.yml", "release-package.yml")),
            dict(reference, path=reference["path"].replace("ci-full.yml", "release-check.yml")),
        ]
        self.job["name"] = "release-check / package / package"
        verify = lambda target: check.proved_run(
            self.api, self.run_data, CI, WORKFLOW_ID, target, CURRENT, check.PACKAGE_WORKFLOW
        )
        self.assertTrue(verify(TREE))
        self.assertFalse(verify(OTHER_TREE))
        self.assertFalse(self.proved())
        self.run_data["referenced_workflows"].pop()
        self.assertFalse(verify(TREE))

    def test_release_pr_dispatch_requires_same_repo_bot_pr(self):
        branch = "release-plz-test"
        self.request.update(user={"id": check.BOT_ID})
        self.request["head"]["ref"] = branch

        def api(endpoint):
            self.assertEqual(endpoint, f"pulls?head=odineska%3A{branch}&base=main&state=all")
            return [self.request]

        ref = "refs/heads/" + branch
        self.assertTrue(check.source_allows_version_check(api, HEAD, ref, "workflow_dispatch"))
        self.assertFalse(check.source_allows_version_check(api, MERGE, ref, "workflow_dispatch"))
        self.request["user"]["id"] = 1
        self.assertFalse(check.source_allows_version_check(api, HEAD, ref, "workflow_dispatch"))

    def test_narrow_path_needs_full_baseline_and_exact_tag(self):
        with (
            patch.object(check, "git", side_effect=[HEAD, OTHER_TREE]) as git,
            patch.object(check, "release_version", return_value="0.35.1"),
            patch.object(check, "find_proof", side_effect=[None, (RUN, ATTEMPT)]) as find,
        ):
            self.assertEqual(
                check.check_mode(None, MAIN, TREE, CURRENT, True, "refs/tags/v0.35.1", "workflow_dispatch"),
                ("package", (RUN, ATTEMPT)),
            )
            self.assertEqual(find.call_args.args[1:], (HEAD, OTHER_TREE, CURRENT))
            self.assertEqual(find.call_args.kwargs, {})
            self.assertEqual(git.call_count, 2)
        with (
            patch.object(check, "git", return_value=HEAD),
            patch.object(check, "release_version", return_value="0.35.1"),
            patch.object(check, "find_proof", return_value=None) as find,
        ):
            self.assertEqual(
                check.check_mode(None, MAIN, TREE, CURRENT, True, "refs/tags/v0.35.2", "workflow_dispatch"),
                ("full", None),
            )
            self.assertEqual(find.call_count, 2)
            self.assertEqual(
                check.check_mode(None, MAIN, TREE, CURRENT, True, "refs/heads/main", "push"),
                ("full", None),
            )


if __name__ == "__main__":
    unittest.main()
