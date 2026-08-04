# 路由级回切集成回归 - 执行计划

1. Locate and reuse existing natural failback, all-open, counting-upstream, and
   reservation test fixtures in `gateway/routes.rs`.
2. Add focused three-provider and five-provider ordered failback tests.
3. Add skip/mixed-state/success-short-circuit/fallback/Ready-cap coverage where
   existing helpers make the behavior deterministic.
4. Run the new route test filter and format the touched file.
5. Do not edit production modules or specs; report missing hooks to coordinator.
