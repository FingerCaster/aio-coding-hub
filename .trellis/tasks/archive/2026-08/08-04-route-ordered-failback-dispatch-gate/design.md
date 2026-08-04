# Dispatch Reservation 与 Provider Gate - 技术设计

`RequestDispatchIntent` remains the request-scoped owner of an ordered target
map and the session trigger reservation. `probe_trigger_for(provider_id)`
decides whether the provider needs a lease; direct targets use the ordinary
common gate.

Claiming a provider lease must not transfer or release the session reservation.
The reservation is atomically committed only when the attempt crosses the
existing transport-send boundary. Pre-send aborts keep it for later targets;
dropping an unconsumed intent releases it through existing RAII behavior.

The implementation must preserve persistence failure rollback and Provider
single-flight semantics.
