#!/usr/bin/env bash
# 下载 Tesseract 中英文词库到 src-tauri/resources/tessdata
# 用途：发布构建启用 ocr feature 前，先填充词库（CI / 打包机联网执行）。
# 说明：.traineddata 为二进制大文件，不入库（见 .gitignore）；本机沙箱网络受限时跳过，由 CI 执行。
set -euo pipefail
# 注意：git 不追踪空目录，CI checkout 后 src-tauri/resources/tessdata 不存在，必须先 mkdir 再解析路径
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DIR="$SCRIPT_DIR/../src-tauri/resources/tessdata"
mkdir -p "$DIR"
BASE="https://github.com/tesseract-ocr/tessdata/raw/main"
for f in chi_sim.traineddata eng.traineddata; do
  echo "下载 $f ..."
  curl -fsSL --retry 5 -o "$DIR/$f" "$BASE/$f"
done
echo "完成：词库已就位 $DIR"
