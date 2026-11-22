# Patched third-party crates

Copies of crates Hummingbird depends on, each changed so it builds and runs for `wasm32-wasip3` under WaOS, and `[patch]`ed in the root `Cargo.toml`. Each keeps its own license; only the files listed changed from the source named.

| Crate | Source | What changed |
|---|---|---|
| `cntp_i18n_core` | crates.io 0.4.1 | `Cargo.toml`: its `gpui` feature on the fork's GPUI (`github.com/waos-dev/zed`) in place of `gpui-unofficial`, so its conversions are to the GPUI Hummingbird builds with. |
| `getrandom-0.3` | crates.io 0.3.4, the Zed port's copy (`waos-dev/zed`, `waos/patched/getrandom-0.3`) | `Cargo.toml`, `src/backends.rs`, `src/backends/wasi_p3.rs` (new): Preview 3 forwarded to getrandom 0.4. |
| `tokio` | crates.io 1.54.1 | `src/runtime/park.rs`: `park_timeout` waits on its condvar on `wasm32-wasip3`. tokio takes a wasm target without the `atomics` feature for one with no threads and sleeps out the timeout with its lock held, so a wake from another thread waited out the whole timeout, minutes for sqlx's pool. `wasm32-wasip3`'s cooperative threads have no `atomics` and wake a condvar's waiter as any target's threads do. |

When upstream moves one of these to a new version, the copy is redone from that version with its change carried over.
