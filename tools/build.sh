#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# esp-idf-sys watches the binding header, but does not watch native source files.
# Wake its build script so CMake can rebuild changed C files incrementally.
touch camera_bridge/include/camera_bridge.h
cargo build "$@"
