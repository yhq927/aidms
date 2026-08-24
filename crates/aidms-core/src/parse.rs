//! 解析层：格式识别 + 资源上限（§3.4 解析隔离）+ 纯文本/CSV/md/PDF/docx/xlsx 文本层提取
//!
//! 设计：技设 §5 主路径中 docx/xlsx 走前端 `mammoth`/`SheetJS`，图片/OCR 走 Rust 侧
//! `tesseract`/RapidOCR（系统库，位于 src-tauri）。本模块落地**无需系统库、可独立验证**的
//! 解析子集：纯文本 / CSV / Markdown / PDF 文本层 / **docx / xlsx 纯文本**（zip+xml 手写，
//! README 声明的「Rust 降级」路线）；并对所有入口做扩展名 + magic 白名单、大小上限校验。
//! `extract_text` 对需外部处理的类型（图片）返回 `Err(Unsupported)` 明确移交 OCR。
//!
//! 解析隔离：docx/xlsx 仅纯文本提取、不渲染 HTML、无脚本执行（§10「不可信文件解析隔离」）。

use std::io::Read;

use thiserror::Error;
use zip::ZipArchive;

/// §3.4 资源上限（解析隔离硬约束，超限即中止并记录失败）
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024; // 50MB
pub const MAX_PAGES: usize = 500;
pub const MAX_CHARS: usize = 2_000_000; // 解析后文本上限，防巨量文本撑爆索引
pub const MAX_PARSE_SECS: u64 = 30;
/// zip 条目总数上限（P1-3：防 zip 炸弹变体——海量小条目撑爆累积量；docx/xlsx 共用）
pub const MAX_ZIP_ENTRIES: usize = 5000;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("不支持的文件类型: {0}")]
    Unsupported(String),
    #[error("文件超过大小上限 {0} 字节")]
    TooLarge(usize),
    #[error("ZIP 条目解压后超过大小上限 {0} 字节（疑似解压炸弹）")]
    ZipTooLarge(usize),
    #[error("解析超时（>{0}s）")]
    Timeout(u64),
    #[error("PDF 超过页数上限 {0} 页")]
    TooManyPages(usize),
    #[error("PDF 解析失败: {0}")]
    Pdf(String),
    #[error("ZIP/XML 解析失败: {0}")]
    Zip(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 解析输入端种类（与技设 §5 一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Txt,
    Markdown,
    Csv,
    Pdf,
    Docx,
    Xlsx,
    Image,
    Business,
}

impl Kind {
    /// 该类型是否需要外部处理（OCR），本模块不直解
    pub fn needs_external(&self) -> bool {
        matches!(self, Kind::Image)
    }
}

/// 扩展名白名单（小写）
pub fn kind_from_ext(ext: &str) -> Option<Kind> {
    match ext.to_ascii_lowercase().as_str() {
        "txt" => Some(Kind::Txt),
        "md" | "markdown" => Some(Kind::Markdown),
        "csv" => Some(Kind::Csv),
        "pdf" => Some(Kind::Pdf),
        "docx" => Some(Kind::Docx),
        "xls" | "xlsx" => Some(Kind::Xlsx),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tif" | "tiff" => Some(Kind::Image),
        _ => None,
    }
}

/// magic 字节白名单（防伪造扩展名）。docx/xlsx 同为 zip，此处不区分，交由扩展名/外部。
pub fn kind_from_magic(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(b"%PDF") {
        return Some(Kind::Pdf);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(b"\xff\xd8\xff") {
        return Some(Kind::Image);
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08") {
        // zip 容器（docx/xlsx）：具体类型由扩展名决定
        return None;
    }
    None
}

/// 资源上限预检（大小）
pub fn check_size(bytes: &[u8]) -> Result<(), ParseError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ParseError::TooLarge(bytes.len()));
    }
    Ok(())
}

/// 统一解析入口：字节 -> 纯文本
///
/// - 纯文本/CSV/Markdown：直接转码或行列拼接
/// - PDF：抽文本层；无文本层（扫描件）返回 `Ok(None)` 交由 OCR
/// - docx/xlsx：zip+xml 纯文本提取（Rust 降级路线）；空文档返回 `Ok(None)`
/// - image：返回 `Err(Unsupported)` 明确移交 Rust OCR
/// - Business：无解析，返回 `Ok(None)`
pub fn extract_text(kind: Kind, bytes: &[u8]) -> Result<Option<String>, ParseError> {
    check_size(bytes)?;
    match kind {
        Kind::Txt | Kind::Markdown => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
        Kind::Csv => Ok(Some(parse_csv(bytes))),
        Kind::Pdf => extract_pdf(bytes),
        // P2-7：docx/xlsx 与 PDF 一致，解析放独立线程跑，超 MAX_PARSE_SECS 中止（防损坏/恶意文件拖死入口）
        // run_with_timeout 返回 Result<Result<_,_>, _>，and_then 展平内层解析错误
        Kind::Docx => run_with_timeout(|b: Vec<u8>| extract_docx(&b), bytes.to_vec(), MAX_PARSE_SECS)
            .and_then(|r| r),
        Kind::Xlsx => run_with_timeout(|b: Vec<u8>| extract_xlsx(&b), bytes.to_vec(), MAX_PARSE_SECS)
            .and_then(|r| r),
        Kind::Image => Err(ParseError::Unsupported(
            "图片需 OCR（Rust 侧 tesseract/RapidOCR）".into(),
        )),
        Kind::Business => Ok(None),
    }
}

/// CSV -> 可读文本（行列以制表符分隔，便于检索；不引入额外依赖）
fn parse_csv(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.lines()
        .map(|l| l.trim_end().replace(',', "\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// PDF 文本层抽取（pdf-extract 纯 Rust，无系统库）。无文本层返回 Ok(None)。
///
/// 资源上限（§3.4）：① 页数 > `MAX_PAGES` 直接中止（lopdf 预检，pdf-extract 已传递依赖）；
/// ② 解析放线程跑，超 `MAX_PARSE_SECS` 中止返回 `Timeout`（超时线程结果丢弃，不阻塞调用方）。
fn extract_pdf(bytes: &[u8]) -> Result<Option<String>, ParseError> {
    // 页数上限预检
    match lopdf::Document::load_mem(bytes) {
        Ok(doc) => {
            let pages = doc.get_pages().len();
            if pages > MAX_PAGES {
                return Err(ParseError::TooManyPages(pages));
            }
        }
        Err(_) => {
            // 页数统计失败（如加密/损坏 PDF）交 pdf-extract 尝试；其失败会转 Pdf 错误
        }
    }
    let text = run_with_timeout(
        |b: Vec<u8>| pdf_extract::extract_text_from_mem(&b),
        bytes.to_vec(),
        MAX_PARSE_SECS,
    )?;
    match text {
        Ok(t) if !t.trim().is_empty() => {
            let t = t.chars().take(MAX_CHARS).collect::<String>();
            Ok(Some(t))
        }
        Ok(_) => Ok(None), // 扫描件/无文本层
        Err(e) => Err(ParseError::Pdf(e.to_string())),
    }
}

/// 在独立线程执行 `f(arg)`，超 `secs` 秒返回 `Timeout`（线程继续跑但结果丢弃——
/// 无进程隔离的务实中止语义，防止恶意/损坏文件拖死解析入口）。
fn run_with_timeout<F, T, A>(f: F, arg: A, secs: u64) -> Result<T, ParseError>
where
    F: FnOnce(A) -> T + Send + 'static,
    A: Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f(arg));
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs))
        .map_err(|_| ParseError::Timeout(secs))
}

// ---------------- docx / xlsx（zip + xml 纯文本提取，Rust 降级路线） ----------------

/// 解压后上限（P1-D 解压炸弹防护）：
/// 压缩包整体 ≤ `MAX_FILE_BYTES`（50MB），但单个条目解压后可能远大于压缩体积
/// （高压缩比 zip 炸弹）。读取前用 `entry.size()`（中央目录记录的解压后大小）预检，
/// 读取时再用 `take` 限长兜底（防伪造 size 元数据），任一超限即报 `ZipTooLarge`。
const MAX_ZIP_ENTRY_BYTES: u64 = MAX_CHARS as u64 + 1;

/// 校验 zip 条目总数（P1-3：防海量小条目 zip 炸弹变体）。
fn check_zip_entry_count(archive: &zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Result<(), ParseError> {
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(ParseError::ZipTooLarge(archive.len()));
    }
    Ok(())
}

/// 校验 zip 条目解压后大小（解压炸弹预检，P1-D）
fn check_zip_entry_size(entry: &zip::read::ZipFile<'_>) -> Result<(), ParseError> {
    if entry.size() > MAX_ZIP_ENTRY_BYTES {
        return Err(ParseError::ZipTooLarge(entry.size() as usize));
    }
    Ok(())
}

/// 从 zip 条目读取文本：`entry.size()` 预检 + `take(MAX_ZIP_ENTRY_BYTES)` 限长兜底。
/// 返回解压后文本（截断检查：超过上限的条目读不满即中止，报 `ZipTooLarge`）。
fn read_zip_entry_text(
    entry: &mut zip::read::ZipFile<'_>,
    what: &str,
) -> Result<String, ParseError> {
    check_zip_entry_size(entry)?;
    let mut buf = Vec::with_capacity(entry.size().min(64 * 1024) as usize);
    entry
        .by_ref()
        .take(MAX_ZIP_ENTRY_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| ParseError::Zip(format!("{what} 读取失败: {e}")))?;
    if buf.len() as u64 >= MAX_ZIP_ENTRY_BYTES {
        // take 截断说明解压后实际超过上限（防御伪造 size 元数据的炸弹）
        return Err(ParseError::ZipTooLarge(buf.len()));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// docx 纯文本提取：读取 `word/document.xml`，抽取 `<w:t>` 文本并按 `<w:p>` 段落换行。
/// 空文档（无任何文本）返回 `Ok(None)`。
fn extract_docx(bytes: &[u8]) -> Result<Option<String>, ParseError> {
    let mut archive =
        ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ParseError::Zip(e.to_string()))?;
    // P1-3：zip 条目总数上限（防海量小条目炸弹）
    check_zip_entry_count(&archive)?;
    let mut entry = archive
        .by_name("word/document.xml")
        .map_err(|e| ParseError::Zip(format!("docx 缺少 word/document.xml: {e}")))?;
    let xml = read_zip_entry_text(&mut entry, "docx document.xml")?;
    let text = docx_xml_to_text(&xml);
    let text = text.chars().take(MAX_CHARS).collect::<String>();
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// 从 docx 的 `word/document.xml` 抽取纯文本：
/// `<w:t>` 之间的字符拼接，`<w:p>`/`<w:tab>`/`<w:br>` 换行，其余标签忽略。
/// 手写 XML 扫描（无需 xml 解析器依赖），仅关注 w:t 文本节点。
fn docx_xml_to_text(xml: &str) -> String {
    let chars: Vec<char> = xml.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut in_text = false;
    let mut i = 0;
    while i < n {
        if chars[i] == '<' {
            // 读取完整标签（含属性）到 '>' 结束
            let mut tag = String::new();
            i += 1;
            while i < n && chars[i] != '>' {
                tag.push(chars[i]);
                i += 1;
            }
            i += 1; // 跳过 '>'
            let trimmed = tag.trim();
            if trimmed.starts_with("</") {
                let name: String = trimmed[2..]
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "w:t" => in_text = false,
                    "w:p" | "w:tab" | "w:br" => push_newline(&mut out),
                    _ => {}
                }
            } else if trimmed.starts_with("<?") || trimmed.starts_with("<!") {
                // 处理指令 / 注释 / DOCTYPE：忽略
            } else {
                // 开标签：trimmed 形如 `w:t` / `w:t xml:space=...` / `w:p/`（不含 '<'），
                // 直接取到空白或 '/' 为止作为标签名
                let name: String = trimmed
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '/')
                    .collect();
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "w:t" => in_text = true,
                    "w:p" | "w:tab" | "w:br" => push_newline(&mut out),
                    _ => {}
                }
            }
        } else if in_text {
            out.push(chars[i]);
            i += 1;
        } else {
            i += 1;
        }
    }
    decode_xml_entities(out)
}

/// xlsx 纯文本提取：读 `xl/sharedStrings.xml`（共享字符串表）+
/// 遍历 `xl/worksheets/sheet*.xml`，按行把单元格文本以制表符拼接、行间换行。
/// 空工作簿返回 `Ok(None)`。
///
/// P1-3 聚合上限：① zip 条目总数 ≤ `MAX_ZIP_ENTRIES`；② 共享字符串表累计字符 ≤ `MAX_CHARS`；
/// ③ 多 sheet 文本**累积过程中**即按 `MAX_CHARS` 提前截断（超限报 `ZipTooLarge`，不做先全量累积再截断）。
fn extract_xlsx(bytes: &[u8]) -> Result<Option<String>, ParseError> {
    let mut archive =
        ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ParseError::Zip(e.to_string()))?;
    // P1-3：zip 条目总数上限（防海量小条目炸弹变体）
    check_zip_entry_count(&archive)?;
    let shared: Vec<String> = match archive.by_name("xl/sharedStrings.xml") {
        Ok(mut entry) => {
            let xml = read_zip_entry_text(&mut entry, "sharedStrings")?;
            xlsx_shared_strings(&xml)?
        }
        Err(_) => Vec::new(), // 无共享字符串表（纯内联/数值单元格）
    };

    let mut parts: Vec<String> = Vec::new();
    let mut total_chars: usize = 0;
    // 遍历工作表条目（sheet1.xml、sheet2.xml …）
    let names: Vec<String> = archive
        .file_names()
        .filter(|n| {
            n.starts_with("xl/worksheets/")
                && n.ends_with(".xml")
                && !n.contains("_rels")
        })
        .map(|n| n.to_string())
        .collect();
    for name in names {
        let mut entry = archive
            .by_name(&name)
            .map_err(|e| ParseError::Zip(format!("{name} 读取失败: {e}")))?;
        let xml = read_zip_entry_text(&mut entry, &name)?;
        let sheet_text = xlsx_sheet_to_text(&xml, &shared);
        if !sheet_text.trim().is_empty() {
            // P1-3：累积过程中按字符数提前截断（join 引入的 '\n' 由最终 take 兜底）
            total_chars += sheet_text.chars().count();
            if total_chars > MAX_CHARS {
                return Err(ParseError::ZipTooLarge(total_chars));
            }
            parts.push(sheet_text);
        }
    }
    let text = parts.join("\n");
    // join 引入的分隔符可能使总量略超 MAX_CHARS，此处仍 take 兜底（防御性）
    let text = text.chars().take(MAX_CHARS).collect::<String>();
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// 解析 `xl/sharedStrings.xml`：每个 `<si>…</si>` 条目内所有 `<t>…</t>` 文本拼接。
/// P1-3：共享字符串累计字符数超过 `MAX_CHARS` 时返回 `ZipTooLarge`（防海量条目内存膨胀）。
fn xlsx_shared_strings(xml: &str) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();
    let mut total: usize = 0;
    for si in xml.split("<si>").skip(1) {
        let si_body = si.split("</si>").next().unwrap_or("");
        let text: String = si_body
            .split("<t")
            .skip(1)
            .filter_map(|part| {
                part.split('>')
                    .nth(1)
                    .and_then(|s| s.split("</t>").next())
            })
            .collect();
        let decoded = decode_xml_entities(text);
        total += decoded.chars().count();
        if total > MAX_CHARS {
            return Err(ParseError::ZipTooLarge(total));
        }
        out.push(decoded);
    }
    Ok(out)
}

/// 解析单个工作表：按 `<row>` 分组，行内单元格文本以制表符分隔。
/// - `t="s"`：`<v>` 为共享字符串下标，查 `shared`
/// - `t="inlineStr"`：`<is><t>…</t></is>` 内联文本
/// - 其余：数值单元格 `<v>…</v>`
fn xlsx_sheet_to_text(xml: &str, shared: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for row in xml.split("<row").skip(1) {
        let body = match row.splitn(2, '>').nth(1) {
            Some(b) => b,
            None => continue,
        };
        let body = body.split("</row>").next().unwrap_or("");
        let mut cells: Vec<String> = Vec::new();
        for cell in body.split("<c").skip(1) {
            let (attrs, rest) = match cell.split_once('>') {
                Some((a, r)) => (a, r),
                None => (cell, ""),
            };
            let is_shared = attrs.split_whitespace().any(|a| a == "t=\"s\"");
            let is_inline = attrs.split_whitespace().any(|a| a == "t=\"inlineStr\"");
            let value = if is_shared {
                let idx: usize = extract_tag_text(rest, "v").trim().parse().unwrap_or(0);
                shared.get(idx).cloned().unwrap_or_default()
            } else if is_inline {
                extract_tag_text(rest, "t")
            } else {
                extract_tag_text(rest, "v")
            };
            cells.push(value);
        }
        // 去尾部空单元格（右对齐表格留白）
        while cells.last().map(|s| s.is_empty()).unwrap_or(false) {
            cells.pop();
        }
        if !cells.is_empty() {
            lines.push(cells.join("\t"));
        }
    }
    lines.join("\n")
}

/// 提取第一个 `<name …>…</name>` 的文本（不处理嵌套，用于单元格 `<v>`/`<t>` 取值）。
fn extract_tag_text(xml: &str, name: &str) -> String {
    let open = format!("<{name}");
    if let Some(pos) = xml.find(&open) {
        let after = &xml[pos + open.len()..];
        if let Some(gt) = after.find('>') {
            let inner = &after[gt + 1..];
            let close = format!("</{name}>");
            if let Some(end) = inner.find(&close) {
                return decode_xml_entities(inner[..end].to_string());
            }
        }
    }
    String::new()
}

/// 追加换行（避免连续空行）
fn push_newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// 解码 XML 常见实体（&amp; &lt; &gt; &quot; &apos;）
fn decode_xml_entities(s: String) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn build_docx(xml: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("word/document.xml", opts).unwrap();
            w.write_all(xml.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn docx_extract_text() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body>
                <w:p><w:r><w:t>重庆智习室科技有限公司</w:t></w:r></w:p>
                <w:p><w:r><w:t>合同编号：HT-2026-001 &amp; 生效</w:t></w:r></w:p>
              </w:body>
            </w:document>"#;
        let bytes = build_docx(xml);
        let text = extract_text(Kind::Docx, &bytes).unwrap().unwrap();
        assert!(text.contains("重庆智习室科技有限公司"));
        assert!(text.contains("HT-2026-001 & 生效"));
        // 段落间有换行
        assert!(text.lines().count() >= 2);
    }

    #[test]
    fn docx_empty_returns_none() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t></w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = build_docx(xml);
        assert!(extract_text(Kind::Docx, &bytes).unwrap().is_none());
    }

    #[test]
    fn docx_invalid_zip_errors() {
        assert!(matches!(
            extract_text(Kind::Docx, b"not a zip"),
            Err(ParseError::Zip(_))
        ));
    }

    /// 构造高压缩比 docx：document.xml 解压后 > MAX_CHARS，用 Deflate 压成小 zip
    /// （真实解压炸弹：压缩包体积很小，解压后巨量文本）。
    fn build_docx_bomb() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("word/document.xml", opts).unwrap();
            let xml = format!(
                "<w:document><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>",
                "A".repeat(MAX_CHARS + 100)
            );
            w.write_all(xml.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn docx_zip_bomb_rejected() {
        // P1-D：高压缩比内容（解压后超上限）必须被拒绝，不能读入内存建索引
        let bytes = build_docx_bomb();
        // 压缩包本身很小（远小于 50MB 前置上限），但解压后超限
        assert!(bytes.len() < 200_000);
        assert!(matches!(
            extract_text(Kind::Docx, &bytes),
            Err(ParseError::ZipTooLarge(_))
        ));
    }

    #[test]
    fn xlsx_zip_bomb_rejected() {
        // P1-D：xlsx 共享字符串表超限同样拒绝
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("xl/sharedStrings.xml", opts).unwrap();
            let xml = format!(
                "<sst>{}</sst>",
                format!("<si><t>{}</t></si>", "B".repeat(MAX_CHARS + 100))
            );
            w.write_all(xml.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        assert!(buf.len() < 200_000);
        assert!(matches!(
            extract_text(Kind::Xlsx, &buf),
            Err(ParseError::ZipTooLarge(_))
        ));
    }

    fn build_xlsx(shared_xml: &str, sheets: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("xl/sharedStrings.xml", opts).unwrap();
            w.write_all(shared_xml.as_bytes()).unwrap();
            for (name, sheet) in sheets {
                w.start_file(name, opts).unwrap();
                w.write_all(sheet.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn xlsx_extract_text_shared_and_numbers() {
        let shared = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <si><t>名称</t></si><si><t>重庆智习室科技有限公司</t></si>
        </sst>"#;
        let sheet1 = r#"<worksheet xmlns="...">
            <sheetData>
              <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
              <row r="2"><c r="A2"><v>2026</v></c><c r="B2"><v>12.4</v></c></row>
            </sheetData>
          </worksheet>"#;
        let bytes = build_xlsx(shared, &[("xl/worksheets/sheet1.xml", sheet1)]);
        let text = extract_text(Kind::Xlsx, &bytes).unwrap().unwrap();
        assert!(text.contains("名称"));
        assert!(text.contains("重庆智习室科技有限公司"));
        assert!(text.contains("2026"));
        assert!(text.contains("12.4"));
        // 单元格制表符分隔
        assert!(text.lines().next().unwrap().contains('\t'));
    }

    #[test]
    fn xlsx_inline_string() {
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="inlineStr"><is><t>内联文本</t></is></c></row>
          </sheetData></worksheet>"#;
        let bytes = build_xlsx("", &[("xl/worksheets/sheet1.xml", sheet)]);
        let text = extract_text(Kind::Xlsx, &bytes).unwrap().unwrap();
        assert!(text.contains("内联文本"));
    }

    #[test]
    fn xlsx_multi_sheet_aggregate_over_limit_rejected() {
        // P1-3：多 sheet 累积超过 MAX_CHARS（单条目未超限）必须被拒绝，
        // 不能「先全量累积再 take 截断」——否则海量 sheet 会先撑爆内存。
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("xl/sharedStrings.xml", opts).unwrap();
            w.write_all(b"<sst></sst>").unwrap();
            // 2500 个 sheet，每个 ~900 字符 → 累积 ~225 万字符 > MAX_CHARS(2,000,000)
            // 条目数 2501 < MAX_ZIP_ENTRIES(5000)：先命中「聚合字符超限」而非条目数超限
            for i in 0..2500 {
                let name = format!("xl/worksheets/sheet{i}.xml");
                w.start_file(name, opts).unwrap();
                let xml = format!(
                    "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>{}</t></is></c></row></sheetData></worksheet>",
                    "A".repeat(900)
                );
                w.write_all(xml.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        // 压缩包整体远小于 50MB 前置上限，但聚合文本超限
        assert!(buf.len() < 3_000_000);
        assert!(matches!(
            extract_text(Kind::Xlsx, &buf),
            Err(ParseError::ZipTooLarge(_))
        ));
    }

    #[test]
    fn xlsx_many_entries_rejected() {
        // P1-3：zip 条目总数超过 MAX_ZIP_ENTRIES 直接拒绝（防海量小条目炸弹变体）
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for i in 0..(MAX_ZIP_ENTRIES + 1) {
                let name = format!("xl/worksheets/sheet{i}.xml");
                w.start_file(name, opts).unwrap();
                w.write_all(b"<worksheet/>").unwrap();
            }
            w.finish().unwrap();
        }
        assert!(matches!(
            extract_text(Kind::Xlsx, &buf),
            Err(ParseError::ZipTooLarge(_))
        ));
    }
}