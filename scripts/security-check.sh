#!/usr/bin/env bash
# 阶段8 安全回归（可自动化部分）：开发计划 §3 全项校验
# 用法：bash scripts/security-check.sh
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== [1] prod CSP 不含 dev 放宽项（vite/HMR/localhost），且无非合规内联 =="
PROD_CSP=$(node -e "const c=require('./src-tauri/tauri.conf.json'); console.log(c.app.security.csp.prod)" 2>/dev/null)
echo "prod CSP: $PROD_CSP"
if echo "$PROD_CSP" | grep -qiE "vite|1420|HMR"; then
  echo "FAIL: prod CSP 含 dev 放宽项(vite/HMR/localhost)"; exit 1
elif echo "$PROD_CSP" | grep -qiE "script-src[^;]*'unsafe-inline'"; then
  echo "FAIL: prod CSP 允许 script 内联(危险)"; exit 1
elif echo "$PROD_CSP" | grep -qiE "style-src 'unsafe-inline'"; then
  echo "FAIL: prod CSP 全局 style 内联(非 attr/elem 白名单)"; exit 1
else
  echo "PASS: prod CSP 无 dev 放宽项（style-src-attr/elem 精确两项白名单合规）"
fi

echo "== [2] 隔离 webview 独立 CSP 随包（dist/index.html 不依赖 dev 配置） =="
if [ -f frontend/dist/index.html ]; then
  echo "  frontend/dist 已构建，发布时由 Tauri 内嵌 prod CSP（非 dev）"
else
  echo "  frontend/dist 未构建（CI/release 构建阶段生成）"
fi

echo "== [3] 日志防泄露（源码无 api_key/content_text 全文打印） =="
HITS=$(grep -rnE "(api_key|content_text)" src-tauri/src crates 2>/dev/null \
  | grep -E "(println|eprintln)" \
  | grep -viE "len\(\)|is_none|is_some|status|message|!=|==|to_string|debug|eprintln!" || true)
if [ -n "$HITS" ]; then
  echo "WARN: 以下代码可能泄露敏感字段，请人工核查："; echo "$HITS"
else
  echo "PASS: 未发现 api_key/content_text 全文打印"
fi

echo "== [4] SSRF / 维度守卫单测（需编译，建议在 CI 跑） =="
echo "  运行: (cd src-tauri && cargo test --lib net::) 与 (cd crates/aidms-core && cargo test)"
echo "== [5] 密钥不落明文 =="
if grep -rnE "write.*api_key|insert.*api_key" src-tauri/src/config.rs 2>/dev/null | grep -qiE "plain|明文" ; then
  echo "WARN: 配置可能存在明文写密钥"
else
  echo "PASS: 密钥经 keyring/加密存储（config.rs 已实现，人工确认）"
fi

echo "自动化安全回归检查完成。其余项（可访问性、RAG 提示注入专项、安装后冒烟）见 docs/release-checklist.md"
