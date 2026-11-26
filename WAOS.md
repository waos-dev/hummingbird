# Hummingbird for WaOS

The branch `waos` of `github.com/waos-dev/hummingbird` is Hummingbird ported to [WaOS](https://github.com/waos-dev/waos): built for `wasm32-wasip3` and run as the WaOS program `dev.waos.hummingbird`. Its library lives in the program's own `/data`, its logs in `/cache/logs`, and the folders it scans are the ones the user picks for it, which WaOS keeps mounted across runs.

What is on top of upstream Hummingbird:

- GPUI from WaOS's fork of Zed (`github.com/waos-dev/zed`), at the revision `wasdk-adapter-gpui` is built on, for every target; on WaOS, `wasdk-adapter-gpui` is GPUI's platform (`ui::app::run`) and `#[wasdk::main]` the program's entry (`main.rs`).
- `src/devices/builtin/waos.rs`: the sound output, WaOS's audio graph: a ring the playback thread fills, as cpal's, and a writer thread that drains it into a `stream-input` node.
- WASI paths in Hummingbird's own code, each marked `WaOS port:`: the program's folders, no Discord presence, no updater, no Opus or HE-AAC decoders yet (their C libraries are not built for WaOS), no mimalloc.
- `waos/patched/`: third-party crates changed for WaOS, `[patch]`ed in `Cargo.toml` (`waos/patched/README.md` says what each is and what changed).
- `waos/env.sh`: the environment a WaOS build needs, from a wasi-sdk.
- `rust-toolchain.toml`: WaOS's nightly, with `wasm32-wasip3`.
- `.github/workflows/waos-release.yml`: the release; upstream's workflows run on `master` alone, so none runs on this branch or its tags.
- `waos/icon.svg`, `waos/icon.png`: Hummingbird's macOS icon, square and filling its canvas, as WaOS draws icons.

The program's `std::thread`s are WaOS's cooperative threads, which take turns on one core: the playback thread, tokio's workers and sqlx's SQLite worker run as written.

## Building

Needed:

- Rust through rustup: `rustup toolchain install` in the checkout installs `rust-toolchain.toml`'s.
- [wasi-sdk 34](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34) or newer, unpacked anywhere: its cooperative-threads libc, which gives the program its threads, and its C compiler, for SQLite.
- `waos`, and a WaOS to run it in, of 0.8.0 or newer: the release whose SDK it builds with, the first to serve its GPU interface, `wasdk:gpu`.

Then, in each shell, with `WASI_SDK` the wasi-sdk's folder (`wasi-sdk-34.0-arm64-macos`, `wasi-sdk-34.0-x86_64-linux`, …):

```sh
WASI_SDK=<folder> . waos/env.sh
waos run --release --no-default-features    # in the WaOS app on this computer
waos package --no-default-features          # target/waos/dev.waos.hummingbird-<version>.wapk and index.json
```

`--no-default-features` leaves out the online services (Last.fm, ListenBrainz), whose reqwest has no WASI backend. A native build of this branch, on the fork's GPUI, is not checked yet.

## Releasing

1. Set the version in `Cargo.toml` and commit it on `waos`.
2. Push a tag `waos-v<that version>`, e.g. `git tag waos-v0.4.0 && git push origin waos-v0.4.0`.

`waos-release.yml` checks the tag against the version, builds and packages the program on Linux, signed with the WaOS project's key (the repository's secret `WAOS_SIGNING_KEY`), and attaches the `.wapk` and its `index.json` to the tag's GitHub release, marked latest. WaOS reads a repository's latest release, so `github.com/waos-dev/hummingbird` added as a source in WaOS installs and updates Hummingbird; the official source, `waos-dev/apps`, lists it from there. A failed release is run again from its run's page.

## Keeping up with upstream Hummingbird

`master` follows `hummingbird-player/hummingbird`; the port lives on `waos`. To take a newer Hummingbird:

```sh
git fetch upstream
git merge upstream/master     # on waos; `git diff upstream/master...waos` is the whole port
```

- Conflicts are in the `WaOS port:` changes.
- Where upstream moves a crate `waos/patched` has a copy of, the copy is redone from the new version with its change carried over, or its `[patch]` no longer applies and cargo says it is unused.
- New code may need its own WASI path: `cargo check --target wasm32-wasip3 --no-default-features`, with `waos/env.sh` sourced, finds it.
