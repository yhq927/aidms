# AIDMS 发布门禁与阶段 8 清单

> 对应《开发计划》阶段 8（自测 + 联调 + 打包）与 §3 安全基线。
> 五轮循环审计已完成（core 64 + tauri 9 测试全绿），本文件聚焦「发布前最后一步」。

## 一、OCR 启用（feature = "ocr"）

OCR 为扫描件增强能力；缺它系统降级为 `ocr_pending`（不阻塞核心入库/搜索）。

### 1.1 词库（随包 resources，二进制不入库）
发布构建前下载中英文词库到 `src-tauri/resources/tessdata/`：
```bash
bash scripts/download-tessdata.sh        # Linux/macOS
# 或 powershell -File scripts/download-tessdata.ps1   # Windows
```
词库经 `bundle.resources: ["resources/tessdata"]` 随包，`ocr.rs` 运行时经
`app.path().resource_dir()/resources/tessdata` 定位（不依赖 cwd）。

### 1.2 编译期：各平台需系统 libtesseract 开发库（链接）
| 平台 | 命令 | 运行时依赖 |
|---|---|---|
| Linux | `apt-get install -y libtesseract-dev libleptonica-dev` | `libtesseract5`（deb depends 已声明） |
| macOS | `brew install tesseract` | 随包 dylib（需打包时带上） |
| Windows | `vcpkg install tesseract:x64-windows` | 随包 tesseract.dll（vcpkg 静态/动态） |

> ⚠️ **跨平台运行时分发仍是发布门禁工程**：tesseract crate 默认动态链接，
> macOS/Windows 需把 `libtesseract.*` 随包（externalBin / resources + 运行时 PATH），
> 否则 OCR 在该平台运行期加载失败 → 自动降级 `ocr_pending`。Linux 用 `deb depends` 最干净。
> CI（`.github/workflows/release.yml`）已对各平台装 dev 库并 `--features ocr` 构建；
> 本机沙箱无 libtesseract，**未验证 ocr feature 编译**，由 CI 三平台 job 兜底。

### 1.3 降级路径（已固化）
未启用 `ocr`（或词库/库缺失）→ 图片/扫描件 PDF 记 `ocr_pending`，前端明确标示「扫描件待 OCR」。

## 二、三平台打包

### 2.1 CI（推荐，真出包）
- 工作流：`.github/workflows/release.yml`，`push: tag v*` 或手动触发。
- 三平台 matrix：`windows-latest` / `macos-latest` / `ubuntu-22.04`，各打各自包。
- 每 job：`setup-node@22` + `dtolnay/rust-toolchain` + 系统 tesseract dev 库 → `download-tessdata.sh` → `tauri-action --features ocr`。
- 产物：`src-tauri/target/release/bundle/**`，upload-artifact。

### 2.2 本机验证状态（沙箱）
- ✅ `cargo check` / `cargo test`（not ocr）全绿
- ✅ `tauri build` 前端集成 + Rust release 编译通过（产出 `target/release/aidms.exe`）
- ❌ 安装包未出：本机缺 **WiX** 与 **NSIS**（bundling 工具）；需在装了对应工具的环境或 CI windows runner 出 `.msi`/`.exe`

### 2.3 代码签名 / 公证（发布必做）
| 平台 | 凭证（仓库 Secrets） |
|---|---|
| Windows | `TAURI_SIGNING_PRIVATE_KEY`（PKCS#8）+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（SmartScreen 信誉随使用累积） |
| macOS | `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD` + `APPLE_ID` + `APPLE_PASSWORD` + `APPLE_TEAM_ID` + `APPLE_SIGNING_IDENTITY`（含 notarization） |
| Linux | 代码签名可选 |

## 三、阶段 8 安全回归（§3 全项）
- [ ] prod CSP 校验 —— `bash scripts/security-check.sh`（自动）
- [ ] SSRF 拦截单测 —— `cargo test --lib net::`（CI 跑，含 IPv6/私网拉黑）
- [ ] 密钥不落明文 —— keyring + Argon2id/AES-GCM（`config.rs`）
- [ ] 解析隔离 —— 资源上限 + 运行时完整性（`parse.rs`：大小/条目数/聚合截断）
- [ ] IPC 越权 —— canonicalize + 越界校验（`commands.rs`）
- [ ] XSS 净化 —— 高亮仅 `<mark>`，DOMPurify 白名单（前端）
- [ ] 日志防泄露 —— 无 `api_key`/`content_text` 全文（`security-check.sh` 自动）
- [ ] RAG 提示注入专项 —— system/user 分角色 + 数据边界标记 + 不带 `tools`（`rag.rs`）

## 四、安装后冒烟（全链路）
1. 启动 → 导入普通 PDF/Word → 搜索命中 → 配置 LLM（Ollama/兼容端点）→ 问答（语义融合）
2. 导入扫描件（图片/无文本 PDF）→ OCR（feature=ocr 时自动识别，否则标 `ocr_pending`）→ 搜索识别文本
3. 多主体：新建主体 → 文件归属 → 三维筛选联动
4. 导出 CSV/JSON
5. 自定义字段：新建字段 → 业务条目填值 → 搜索字段值

## 五、G6 统计机制（发布门禁项，开发计划阶段 8.6）
- **主体标注率**：统计机制 + 展示（上线后真实统计，不卡阈值）
- **筛选准确率**：≥50 样本 ground-truth 比对 ≥95%（仅校验筛选逻辑按选定维度正确返回，不校验标签对错）
- 状态见 `README.md`「G6」段

## 六、已知限制（不阻塞主流程）
1. OCR 运行时 tesseract 动态库跨平台分发（Windows/macOS 随包处理）
2. 三平台真出包需 CI + 签名证书/Apple 开发者账号
3. 慢 prefill 首 token 60s 读间隔超时（N1，已文档化）
4. 持锁嵌入补偿（P2-10，README 已记录）
5. 拖拽导入（tauri://drag-drop）未接线（当前走 dialog 主链路）
