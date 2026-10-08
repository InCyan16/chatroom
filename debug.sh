#!/bin/sh
set -eu
cd "$(dirname "$0")"
if [ ! -x target/debug/chat ]; then
    printf '%s\n' 'Debug artifact missing. Run ./build.sh debug first.' >&2
    exit 1
fi
exec ./target/debug/chat
