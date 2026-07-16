# Decision record

The plugin capability review and managed native integration were implemented
and validated in the integration branch. Real WebView bridge behavior then
demonstrated that AIO would own a large, security-sensitive compatibility
surface for an external project.

The user changed the product decision on 2026-07-16: remove the native
integration and keep only a repository recommendation. This eliminates source
trust, process ownership, route coordination, bridge security, update, and UI
compatibility maintenance from AIO while still making the external project
discoverable.
