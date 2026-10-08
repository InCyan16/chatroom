#!/bin/sh
set -eu
cd "$(dirname "$0")"
mode=${1:-release}
case "$mode" in
    debug|release) ;;
    *) printf '%s\n' 'Usage: ./build.sh [debug|release]' >&2; exit 1 ;;
esac
if [ -f web/pnpm-lock.yaml ]; then
    pnpm --dir web install --frozen-lockfile
else
    pnpm --dir web install
fi
if [ "$mode" = debug ]; then
    NODE_ENV=development pnpm --dir web build --mode development --outDir dist-debug
else
    pnpm --dir web build
fi
if [ ! -f Cargo.lock ]; then
    cargo generate-lockfile
fi
if [ "$mode" = debug ]; then
    cargo build --locked
    printf '%s\n' 'Built target/debug/chat. Run ./debug.sh.'
else
    cargo build --release --locked
    cp target/release/chat ./chat
    printf '%s\n' 'Built ./chat with the frontend embedded.'
fi
