#!/bin/bash
# cmake wrapper for musl cross-compilation
# - Adds -fno-stack-protector to prevent __stack_chk_fail undefined reference
# - Adds CMAKE_POLICY_VERSION_MINIMUM=3.5 for cmake 4.x compatibility
set -euo pipefail

cmake_args=()
for arg in "$@"; do
    case "$arg" in
        -DCMAKE_C_FLAGS=*)
            cmake_args+=("${arg} -fno-stack-protector")
            ;;
        -DCMAKE_CXX_FLAGS=*)
            cmake_args+=("${arg} -fno-stack-protector")
            ;;
        -DCMAKE_ASM_FLAGS=*)
            cmake_args+=("${arg} -fno-stack-protector")
            ;;
        *)
            cmake_args+=("$arg")
            ;;
    esac
done

exec cmake -DCMAKE_POLICY_VERSION_MINIMUM=3.5 "${cmake_args[@]}"
