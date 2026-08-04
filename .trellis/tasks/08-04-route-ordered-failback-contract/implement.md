# 回切路由契约更新 - 执行计划

1. Diff the current contract against the parent PRD/design.
2. Update signatures and ordinary failback rules without removing unrelated
   all-open, security, projection, or retry guarantees.
3. Extend validation matrix, examples, tests, and wrong/correct snippets.
4. Search for obsolete single-target wording and run Markdown/diff checks.
5. Do not edit Rust source or route tests.
