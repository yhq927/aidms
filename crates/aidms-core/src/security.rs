//! 安全基线公共 util（阶段 1 即落地，全程复用）
//!
//! - [`canonicalize_safe`]：规范化路径并确认落在允许根目录内，防 `..` 逃逸 / symlink 穿越
//! - [`redact_log`]：日志脱敏，避免 `api_key` / `content_text` 全文泄露（开发期默认生效）
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum SecurityError {
    RootNotExists,
    InvalidPath,
    EscapeAllowedRoot,
}

/// 规范化路径并确认落在允许根目录内。
///
/// 先 `canonicalize` 允许根（必须存在），再把目标 join 到根下并 `canonicalize`
/// （跟随 symlink 与 `..`），最后校验规范化结果仍以根为前缀。用于拖拽导入 /
/// 文件夹监控预授权目录的越界校验。
pub fn canonicalize_safe(path: &str, allowed_root: &str) -> Result<PathBuf, SecurityError> {
    let root = Path::new(allowed_root);
    let root_canon = root.canonicalize().map_err(|_| SecurityError::RootNotExists)?;
    let joined = root_canon.join(path);
    let canon = joined.canonicalize().map_err(|_| SecurityError::InvalidPath)?;
    if !canon.starts_with(&root_canon) {
        return Err(SecurityError::EscapeAllowedRoot);
    }
    Ok(canon)
}

/// 日志脱敏：按空白切分，对疑似密钥/超长 token 掩码或截断。
pub fn redact_log(msg: &str) -> String {
    msg.split_whitespace()
        .map(|tok| {
            if is_secret_like(tok) {
                "***".to_string()
            } else if tok.len() > 120 {
                format!("{}…", &tok[..120])
            } else {
                tok.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_secret_like(tok: &str) -> bool {
    tok.starts_with("sk-")
        || tok.starts_with("Bearer ")
        || tok.contains("api_key")
        || tok.contains("apikey")
        || tok.contains("password")
        || tok.contains("secret")
        || (tok.len() >= 32 && tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aidms_sec_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn canonicalize_inside_root_ok() {
        let root = tmp_root();
        let f = root.join("file.txt");
        std::fs::write(&f, "x").unwrap();
        let out = canonicalize_safe("file.txt", root.to_str().unwrap()).unwrap();
        assert!(out.ends_with("file.txt"));
    }

    #[test]
    fn canonicalize_rejects_escape() {
        let root = tmp_root();
        // 跨平台逃逸构造：先建一个真实存在的根外文件，再尝试从根内用相对路径越过根。
        // （Linux 专属的 /etc/passwd 在 Windows 上不存在，不能作为测试锚点）
        let outside = std::env::temp_dir().join(format!("aidms_sec_outside_{}", std::process::id()));
        std::fs::write(&outside, "x").unwrap();
        let rel = format!("../{}", outside.file_name().unwrap().to_string_lossy());
        let res = canonicalize_safe(&rel, root.to_str().unwrap());
        assert_eq!(res, Err(SecurityError::EscapeAllowedRoot));
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn redact_masks_secret() {
        let out = redact_log("call api_key=sk-1234567890abcdef payload=ok");
        assert!(!out.contains("sk-1234567890abcdef"));
        assert!(out.contains("***"));
        assert!(out.contains("payload=ok"));
    }
}
