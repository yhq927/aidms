//! OCR 调度（Rust 侧 tesseract；`feature = "ocr"` 启用）
//!
//! 技设 §5：图片/扫描件 PDF 走 Rust 侧 OCR，避开 webview WASM/Worker/COOP-COEP/CSP 坑。
//! 默认 `tesseract` crate（依赖系统 libtesseract + `chi_sim.traineddata`）；
//! 若系统库缺失/链接失败，降级 RapidOCR（`ort` + PP-OCRv4 ONNX，更易随包分发）。
//!
//! `TESSDATA_PREFIX` 指向随包分发的词库目录（打包时放入 resources，设 `TESSDATA_PREFIX` 或
//! 直接在 `Tesseract::new` 传 datapath）。中文优先 `chi_sim+eng`。
//!
//! ⚠️ P1-6 OCR 端到端状态（第 2 轮审计）：
//! - `tauri.conf.json` bundle 已加 `resources: []` 占位，阶段 8 打包时加入
//!   `tessdata/*.traineddata`（含 chi_sim）+ ONNX（若启用 RapidOCR）。
//! - **RapidOCR 降级为 TODO**：需引入 `ort` 运行时 + PP-OCRv4 ONNX 模型（见技术设计 §5），
//!   当前不引入无法在沙箱验证的依赖；tesseract 路径（feature=ocr）保持可用。
//! - 未启用 feature=ocr 时，图片保持 `ocr_pending` 状态，前端明确标示「扫描件待 OCR」。
#![cfg(feature = "ocr")]

use std::path::Path;
use tesseract::Tesseract;

/// 对单张图片做 OCR，返回识别文本。
pub fn ocr_image(image_path: &Path, tessdata_prefix: &Path, lang: &str) -> Result<String, String> {
    let mut t = Tesseract::new(Some(tessdata_prefix), lang)
        .map_err(|e| format!("tesseract 初始化失败: {e}"))?;
    t.set_image(image_path)
        .map_err(|e| format!("设置图像失败: {e}"))?;
    t.recognize().map_err(|e| format!("识别失败: {e}"))?;
    t.get_text().map_err(|e| format!("获取文本失败: {e}"))
}

/// 批量 OCR（多页扫描件/多图）：逐页识别并拼接。
pub fn ocr_images(
    images: &[&Path],
    tessdata_prefix: &Path,
    lang: &str,
) -> Result<String, String> {
    let mut out = String::new();
    for (i, img) in images.iter().enumerate() {
        let text = ocr_image(img, tessdata_prefix, lang)?;
        out.push_str(&format!("\n--- 第 {} 页 ---\n{}", i + 1, text));
    }
    Ok(out)
}
