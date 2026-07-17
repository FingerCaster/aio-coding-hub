from __future__ import annotations

import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

from common.task_context import validate_all_context_manifests
from common.task_store import _rewrite_archived_context_paths, cmd_archive


class ArchiveContextRewriteTests(unittest.TestCase):
    def test_archive_rewrites_only_self_references_and_full_validation_passes(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            repo_root = Path(raw_root)
            original = repo_root / ".trellis" / "tasks" / "07-17-example"
            archived = (
                repo_root
                / ".trellis"
                / "tasks"
                / "archive"
                / "2026-07"
                / "07-17-example"
            )
            research = archived / "research" / "evidence.md"
            research.parent.mkdir(parents=True)
            research.write_text("evidence", encoding="utf-8")
            shared = repo_root / ".trellis" / "spec" / "shared.md"
            shared.parent.mkdir(parents=True)
            shared.write_text("shared", encoding="utf-8")
            rows = [
                {
                    "file": ".trellis/tasks/07-17-example/research/evidence.md",
                    "reason": "self",
                    "type": "file",
                    "future": {"preserve": True},
                },
                {
                    "file": ".trellis/tasks/07-17-example",
                    "reason": "exact root",
                },
                {
                    "file": ".trellis/tasks/07-17-example-other/evidence.md",
                    "reason": "similar prefix",
                },
                {"file": ".trellis/spec/shared.md", "reason": "other"},
            ]
            for name in ("implement.jsonl", "check.jsonl"):
                (archived / name).write_text(
                    "\n".join(json.dumps(row) for row in rows) + "\n",
                    encoding="utf-8",
                )

            _rewrite_archived_context_paths(original, archived, repo_root)

            for name in ("implement.jsonl", "check.jsonl"):
                rewritten = [
                    json.loads(line)
                    for line in (archived / name).read_text(encoding="utf-8").splitlines()
                ]
                self.assertEqual(
                    rewritten[0]["file"],
                    ".trellis/tasks/archive/2026-07/07-17-example/research/evidence.md",
                )
                self.assertEqual(
                    rewritten[1]["file"],
                    ".trellis/tasks/archive/2026-07/07-17-example",
                )
                self.assertEqual(
                    rewritten[2]["file"],
                    ".trellis/tasks/07-17-example-other/evidence.md",
                )
                self.assertEqual(rewritten[3]["file"], ".trellis/spec/shared.md")
                self.assertEqual(rewritten[0]["type"], "file")
                self.assertEqual(rewritten[0]["future"], {"preserve": True})
                # A task root exercises exact-prefix rewriting, but JSONL targets must
                # ultimately be files. Remove that synthetic row before full validation.
                (archived / name).write_text(
                    "\n".join(json.dumps(row) for row in (rewritten[0], *rewritten[2:]))
                    + "\n",
                    encoding="utf-8",
                )
            similar = repo_root / ".trellis" / "tasks" / "07-17-example-other" / "evidence.md"
            similar.parent.mkdir(parents=True)
            similar.write_text("similar", encoding="utf-8")
            self.assertEqual(validate_all_context_manifests(repo_root), 0)

    def test_archive_command_skips_commit_when_context_json_is_malformed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            repo_root = Path(raw_root)
            task = repo_root / ".trellis" / "tasks" / "07-17-malformed"
            task.mkdir(parents=True)
            (task / "implement.jsonl").write_text("{not-json}\n", encoding="utf-8")

            with (
                patch("common.task_store.get_repo_root", return_value=repo_root),
                patch("common.task_store._auto_commit_archive") as auto_commit,
            ):
                result = cmd_archive(Namespace(name="07-17-malformed", no_commit=False))

            self.assertEqual(result, 1)
            auto_commit.assert_not_called()

    def test_archive_command_skips_commit_when_any_context_target_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            repo_root = Path(raw_root)
            task = repo_root / ".trellis" / "tasks" / "07-17-missing"
            task.mkdir(parents=True)
            row = {
                "file": ".trellis/tasks/07-17-missing/research/missing.md",
                "reason": "missing after archive",
            }
            (task / "implement.jsonl").write_text(
                json.dumps(row) + "\n", encoding="utf-8"
            )

            with (
                patch("common.task_store.get_repo_root", return_value=repo_root),
                patch("common.task_store._auto_commit_archive") as auto_commit,
            ):
                result = cmd_archive(Namespace(name="07-17-missing", no_commit=False))

            self.assertEqual(result, 1)
            auto_commit.assert_not_called()


if __name__ == "__main__":
    unittest.main()
