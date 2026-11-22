# WaOS port: the environment a WaOS build of Hummingbird needs, from a wasi-sdk 34 or newer at
# $WASI_SDK. Source it, then build: `WASI_SDK=<folder> . waos/env.sh && waos run --release
# --no-default-features`.
#
# The program's std::threads are WaOS's cooperative threads, from wasi-sdk's cooperative-threads
# libc in place of Rust's own, and its C code (SQLite) is built against the same sysroot.
# `tokio_unstable` lets tokio build its runtime's features for wasm.

if [ -z "${WASI_SDK:-}" ] || [ ! -d "$WASI_SDK/share/wasi-sysroot/experimental-coop-threads" ]; then
  echo "waos/env.sh: set WASI_SDK to a wasi-sdk 34 or newer, the folder holding bin/ and share/" >&2
  return 1 2>/dev/null || exit 1
fi

coop_sysroot="$WASI_SDK/share/wasi-sysroot/experimental-coop-threads"
coop_libs="$coop_sysroot/lib/wasm32-wasip3"

export CC_wasm32_wasip3="$WASI_SDK/bin/clang"
export CXX_wasm32_wasip3="$WASI_SDK/bin/clang++"
export AR_wasm32_wasip3="$WASI_SDK/bin/llvm-ar"
export CFLAGS_wasm32_wasip3="--sysroot=$coop_sysroot"
export CXXFLAGS_wasm32_wasip3="--sysroot=$coop_sysroot"
export CARGO_TARGET_WASM32_WASIP3_RUSTFLAGS="-C link-self-contained=no -L native=$coop_libs -C link-arg=$coop_libs/crt1-command.o --cfg tokio_unstable"

unset coop_sysroot coop_libs
