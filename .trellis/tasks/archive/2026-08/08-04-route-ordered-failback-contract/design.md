# 回切路由契约更新 - 技术设计

Edit only the existing cross-layer Gateway contract. Replace stale signatures
and single-target language with the ordered-target model while retaining
unrelated security, route projection, retry, and all-open recovery clauses.

The contract must be executable: include exact ordering, trigger, reservation,
stop/continue, and budget rules plus corresponding test matrix entries.
