#!/usr/bin/env bash
set -euo pipefail
NCS_ROOT="${NCS_ROOT:-/opt/nordic/ncs/v3.4.1}"
TOOLCHAIN="${NCS_TOOLCHAIN:-/opt/nordic/ncs/toolchains/97226bad62}"
export PATH="$TOOLCHAIN/bin:$TOOLCHAIN/opt/zephyr-sdk/gnu/arm-zephyr-eabi/bin:$PATH"
export ZEPHYR_TOOLCHAIN_VARIANT=zephyr/gnu
export ZEPHYR_SDK_INSTALL_DIR="$TOOLCHAIN/opt/zephyr-sdk"
# shellcheck source=/dev/null
source "$NCS_ROOT/zephyr/zephyr-env.sh"
