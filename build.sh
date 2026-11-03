#!/usr/bin/env bash
# 构建 Pong 示例到 platforms/douyin/package/。
#
# 真正的构建逻辑在适配层里（addons/bevy-douyin/tools/build.sh），这里只把
# 本仓库的目录布局翻译成它的参数。额外参数会原样传给适配层，例如：
#
#   ./build.sh --no-minify
#   INITIAL_MEMORY_BYTES=$((32*1024*1024)) ./build.sh
set -euo pipefail
cd "$(dirname "$0")"

ADAPTER=addons/bevy-douyin

if [ ! -f "$ADAPTER/tools/build.sh" ]; then
  echo "找不到适配层：$ADAPTER/tools/build.sh" >&2
  echo "先执行：git submodule update --init --recursive" >&2
  exit 1
fi

exec "$ADAPTER/tools/build.sh" \
  --crate platforms/douyin \
  --out platforms/douyin/package \
  --name pong \
  --wasm pong_douyin \
  "$@"
