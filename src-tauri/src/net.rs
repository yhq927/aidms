//! 唯一出网 HTTP 客户端（安全基线）
//!
//! - 禁用自动重定向：避免重定向把请求拐到内网/受限地址（每跳都需重校验）
//! - SSRF 防护：目标 host 必须命中显式白名单；解析出的 IP 禁止指向私有/链路本地/未指定
//!   （技术设计 §10：**本地模式仅放行 `127.0.0.1` 与 `::1`（localhost 双栈）的 Ollama**，
//!   故环回地址不属于受限集；私有/链路本地/未指定仍拉黑，防 SSRF 打内网）
//! - 阶段 3 嵌入调用 / 阶段 5 LLM 调用强制复用本客户端，禁止另行直连
use std::collections::HashSet;
use std::net::{IpAddr, ToSocketAddrs};

use url::Url;

pub struct SafeHttpClient {
    client: reqwest::blocking::Client,
    /// 异步客户端（流式问答用，技术设计 §4/§10：SSRF 防护同样适用）
    async_client: reqwest::Client,
    /// 允许的目标主机白名单（如用户配置的 Ollama 主机、兼容 API 域名）
    allowed_hosts: HashSet<String>,
}

impl SafeHttpClient {
    /// 连接超时 / 总超时（P1-4）：嵌入是阻塞调用且发生在持 DB 锁的事务内，端点无响应时
    /// 单次嵌入挂起会阻塞全应用所有 DB 操作；此处强制 10s 建连 + 120s 总超时，
    /// 超时失败走既有降级（嵌入不可达 → 仅 FTS5），不阻塞入库。
    /// 注（R5-P2-1）：**阻塞客户端**保留 120s 总超时（嵌入/非流式调用）；
    /// **异步客户端**（仅 `post_stream` 流式问答用）去掉总超时——`reqwest` async 的
    /// `.timeout()` 是「建连到读完」的总时长，长答案/慢 prefill 超过 2 分钟会被截断。
    /// 流式读取的「读间隔超时」由调用方（rag.rs `ask_rag`）用 `tokio::time::timeout`
    /// 包每个 chunk 实现：60s 无新数据才超时，超长答案不再截断、挂起仍可退出。
    pub fn new(allowed_hosts: HashSet<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("构建安全 HTTP 客户端失败");
        let async_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            // 无总超时：流式问答可长时间输出（超时控制由调用方按 chunk 读间隔做）
            .build()
            .expect("构建安全异步 HTTP 客户端失败");
        Self {
            client,
            async_client,
            allowed_hosts,
        }
    }

    fn assert_safe_host(&self, host: &str) -> Result<(), String> {
        if !self.allowed_hosts.contains(host) {
            return Err(format!("SSRF 防护：目标主机不在白名单: {host}"));
        }
        // 白名单显式包含的 host 才继续做 IP 受限检查（本地 Ollama 的 127.0.0.1/::1 由
        // is_restricted_ip 摘出环回后天然放行，满足技术设计 §10「本地模式放行 localhost 双栈」）。
        if let Ok(addrs) = format!("{host}:443").to_socket_addrs() {
            for addr in addrs {
                if is_restricted_ip(addr.ip()) {
                    return Err(format!("SSRF 防护：目标解析到受限地址 {addr}"));
                }
            }
        }
        // TODO(P0-1 强化项)：非 loopback host 目前「校验但直连原 host」，未做「解析→钉死 IP 直连」
        // （防 TOCTOU/DNS 重绑）。后续可用 reqwest 的 `.resolve(host, ip)` 把连接钉死到已校验 IP；
        // https 场景需按 IP 校验证书或文档化说明该限制。本地 Ollama（loopback）主场景已放行，不阻塞。
        Ok(())
    }

    pub fn get_text(&self, url: &str) -> Result<String, String> {
        let host = extract_host(url)?;
        self.assert_safe_host(&host)?;
        self.client
            .get(url)
            .send()
            .map_err(|e| e.to_string())?
            .text()
            .map_err(|e| e.to_string())
    }

    pub fn post_json(&self, url: &str, body: &str) -> Result<String, String> {
        let host = extract_host(url)?;
        self.assert_safe_host(&host)?;
        self.client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| e.to_string())?
            .text()
            .map_err(|e| e.to_string())
    }

    /// 带 Bearer 鉴权的 POST（嵌入/LLM 调用，密钥不落日志）
    pub fn post_json_auth(
        &self,
        url: &str,
        body: &str,
        api_key: &str,
    ) -> Result<String, String> {
        let host = extract_host(url)?;
        self.assert_safe_host(&host)?;
        let mut req = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string());
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        req.send().map_err(|e| e.to_string())?
            .text()
            .map_err(|e| e.to_string())
    }

    /// 异步流式 POST（问答 SSE 用）。同样经 SSRF 白名单 + 禁用自动重定向；
    /// 返回 `reqwest::Response`，调用方用 `.bytes_stream()` 逐块读取。密钥仅经 Bearer 发送，不写日志。
    pub async fn post_stream(
        &self,
        url: &str,
        body: &str,
        api_key: &str,
    ) -> Result<reqwest::Response, String> {
        let host = extract_host(url)?;
        self.assert_safe_host(&host)?;
        let mut req = self
            .async_client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string());
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        req.send().await.map_err(|e| e.to_string())
    }
}

/// 按 provider 构造嵌入端点 URL（P0-5 云端模式修复；P1-4 /v1 归一）：
///
/// - `ollama`（本地）：`{base}/api/embed`（保持原样，不补 /v1）
/// - `openai_compat`（云端，如 OpenAI/DashScope/硅基流动）：与 chat 端一致的 /v1 归一
///   （trim 尾斜杠 → 若不含 `/v1` 则补 `/v1` → `+/embeddings`）。
///   修复前用户填 `https://api.siliconflow.cn`（无 /v1）会出现「chat 可用 embed 404」，
///   现与 chat 端点（rag.rs `{base}/v1/chat/completions`）保持一致。
///
/// 调用方 base_url 语义：Ollama 填 `http://127.0.0.1:11434`；OpenAI 兼容填
/// `https://api.openai.com/v1`（含 /v1）或 `https://api.siliconflow.cn`（不含 /v1，自动补）。
pub fn embed_url_for(provider: &str, base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    match provider {
        "ollama" => format!("{base}/api/embed"),
        // 默认按 OpenAI 兼容处理（openai_compat / 未知名 provider 均走该分支）
        _ => {
            let v1 = if base.ends_with("/v1") {
                base.to_string()
            } else {
                format!("{base}/v1")
            };
            format!("{v1}/embeddings")
        }
    }
}

/// 按 OpenAI 兼容/ollama 语义构造 chat completions URL（P2-6：从 src-tauri/rag.rs 抽公共函数）。
///
/// - base_url 已含 `/v1`（如 `https://api.openai.com/v1`）→ `{base}/chat/completions`（去重避免 /v1/v1）
/// - base_url 不含 `/v1`（如 Ollama `http://127.0.0.1:11434`、硅基流动 `https://api.siliconflow.cn`）
///   → `{base}/v1/chat/completions`（Ollama 提供 /v1 兼容端点）
pub fn chat_url_for(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

/// 解析嵌入响应，兼容两种结构：
///
/// - OpenAI 兼容：`{"data": [{"embedding": [f32, ...]}]}`
/// - Ollama：`{"embeddings": [[f32, ...], ...]}`
pub fn parse_embed_response(resp: &str) -> Result<Vec<f32>, String> {
    let v: serde_json::Value =
        serde_json::from_str(resp).map_err(|e| format!("嵌入响应解析失败: {e}"))?;
    // OpenAI 兼容：data[0].embedding（优先，云端主路径）
    if let Some(arr) = v
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.get("embedding"))
        .and_then(|e| e.as_array())
    {
        return arr
            .iter()
            .map(|x| {
                x.as_f64()
                    .map(|f| f as f32)
                    .ok_or_else(|| "向量元素非数值".to_string())
            })
            .collect();
    }
    // Ollama：embeddings[0]
    if let Some(arr) = v
        .get("embeddings")
        .and_then(|e| e.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_array())
    {
        return arr
            .iter()
            .map(|x| {
                x.as_f64()
                    .map(|f| f as f32)
                    .ok_or_else(|| "向量元素非数值".to_string())
            })
            .collect();
    }
    Err("嵌入向量缺失（响应不含 data[].embedding 或 embeddings）".into())
}

/// 调用嵌入模型，返回 f32 向量（P0-5：按 provider 分路）。
///
/// 必须经 [`SafeHttpClient`]（SSRF host 白名单 + 禁用自动重定向）。`api_key` 仅经 Bearer 发送，不写日志。
pub fn embed_text(
    client: &SafeHttpClient,
    provider: &str,
    base_url: &str,
    model: &str,
    api_key: &str,
    text: &str,
) -> Result<Vec<f32>, String> {
    let url = embed_url_for(provider, base_url);
    // Ollama 与 OpenAI 兼容均接受 {model, input} 结构（OpenAI /v1/embeddings 用 "input" 数组）
    let body = serde_json::json!({ "model": model, "input": [text] }).to_string();
    let resp = client.post_json_auth(&url, &body, api_key)?;
    parse_embed_response(&resp)
}

fn extract_host(url: &str) -> Result<String, String> {
    let u = Url::parse(url).map_err(|e| format!("URL 解析失败: {e}"))?;
    u.host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| "URL 缺少 host".to_string())
}

/// 私有 / 链路本地 / 未指定 地址视为受限（SSRF 拉黑）。
///
/// ⚠️ **环回（127.0.0.1/::1）不属于受限**：技术设计 §10「本地模式仅放行 127.0.0.1 与 ::1
/// （localhost 双栈）的 Ollama」——本地 Ollama 默认绑 127.0.0.1:11434，必须放行；
/// 若把环回也拉黑会反向误封设计主用场景（审计 P0-1）。私有/链路本地/未指定仍拉黑，
/// 防 SSRF 把请求拐到内网/云元数据地址（169.254.169.254 等）。
/// IPv6 分支（P1-C）：除未指定外，补拉黑链路本地 fe80::/10（`is_unicast_link_local`）、
/// 唯一本地 fc00::/7（`is_unique_local`）；IPv4 映射段 ::ffff:0:0/96 经 `to_ipv4_mapped`
/// 转回 V4 按 V4 规则判定（如 ::ffff:169.254.169.254 → 169.254.169.254 命中链路本地拉黑）；
/// ::ffff:127.0.0.1 映射回环地址 → 与 127.0.0.1 一致放行（本地 Ollama 双栈语义）。
fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return false; // 本地 Ollama 放行 ::1（技术设计 §10）
            }
            v6.is_unspecified()
                || v6.is_unicast_link_local() // fe80::/10
                || v6.is_unique_local() // fc00::/7
                || v6
                    .to_ipv4_mapped() // ::ffff:0:0/96 映射段
                    .map_or(false, |v4| is_restricted_ip(IpAddr::V4(v4)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn loopback_not_restricted_local_ollama_ok() {
        // 技术设计 §10：本地模式放行 127.0.0.1 / ::1（localhost 双栈 Ollama）
        assert!(!is_restricted_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_restricted_ip(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn private_linklocal_unspecified_still_restricted() {
        // 私网 / 链路本地（云元数据 169.254.169.254）/ 未指定 仍拉黑
        assert!(is_restricted_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(is_restricted_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(is_restricted_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_restricted_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
        assert!(is_restricted_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    }

    #[test]
    fn ipv6_restricted_segments_blacklisted() {
        // P1-C：IPv6 链路本地 fe80::/10、唯一本地 fc00::/7、IPv4 映射段均拉黑
        assert!(is_restricted_ip("fe80::1".parse().unwrap()));
        assert!(is_restricted_ip("fc00::1".parse().unwrap()));
        assert!(is_restricted_ip("fd12:3456::1".parse().unwrap()));
        assert!(is_restricted_ip("::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_restricted_ip("::ffff:10.0.0.5".parse().unwrap()));
        assert!(is_restricted_ip("::".parse().unwrap()));
    }

    #[test]
    fn ipv6_loopback_and_mapped_loopback_allowed() {
        // 技术设计 §10：本地 Ollama 放行 ::1；::ffff:127.0.0.1 映射回环同样放行
        assert!(!is_restricted_ip(std::net::Ipv6Addr::LOCALHOST.into()));
        assert!(!is_restricted_ip("::ffff:127.0.0.1".parse().unwrap()));
        // 公网 IPv6 不应被误拉黑
        assert!(!is_restricted_ip("2400:3200::1".parse().unwrap()));
    }

    #[test]
    fn whitelisted_localhost_passes_assert() {
        let client = SafeHttpClient::new(
            ["127.0.0.1".to_string(), "localhost".to_string()]
                .into_iter()
                .collect(),
        );
        // 白名单显式包含的本地 host：解析到环回地址应放行（本地 Ollama 主场景）
        assert!(client.assert_safe_host("127.0.0.1").is_ok());
        assert!(client.assert_safe_host("localhost").is_ok());
        // 白名单外的 host 仍被拒
        assert!(client.assert_safe_host("example.com").is_err());
    }

    #[test]
    fn non_whitelisted_private_blocked() {
        let client = SafeHttpClient::new(HashSet::new());
        assert!(client.assert_safe_host("192.168.1.10").is_err());
    }

    #[test]
    fn embed_url_openai_compat_v1_normalized() {
        // P1-4：openai_compat 无 /v1 时自动补 /v1（修复「chat 可用 embed 404」）
        assert_eq!(
            embed_url_for("openai_compat", "https://api.siliconflow.cn"),
            "https://api.siliconflow.cn/v1/embeddings"
        );
        // 已含 /v1 不重复补
        assert_eq!(
            embed_url_for("openai_compat", "https://api.openai.com/v1"),
            "https://api.openai.com/v1/embeddings"
        );
        // 尾斜杠先 trim 再判断
        assert_eq!(
            embed_url_for("openai_compat", "https://api.openai.com/v1/"),
            "https://api.openai.com/v1/embeddings"
        );
        assert_eq!(
            embed_url_for("openai_compat", "https://api.deepseek.com/"),
            "https://api.deepseek.com/v1/embeddings"
        );
        // 未知名 provider 默认走 openai_compat 分支
        assert_eq!(
            embed_url_for("unknown", "https://example.com"),
            "https://example.com/v1/embeddings"
        );
    }

    #[test]
    fn embed_url_ollama_unchanged() {
        // P1-4：ollama 分支不变（/api/embed，不补 /v1）
        assert_eq!(
            embed_url_for("ollama", "http://127.0.0.1:11434"),
            "http://127.0.0.1:11434/api/embed"
        );
        assert_eq!(
            embed_url_for("ollama", "http://127.0.0.1:11434/"),
            "http://127.0.0.1:11434/api/embed"
        );
        // ollama 即使误填 /v1 也保持原语义（本地端点不受影响）
        assert_eq!(
            embed_url_for("ollama", "http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/v1/api/embed"
        );
    }

    #[test]
    fn parse_embed_response_openai_and_ollama_structures() {
        // P2-9：兼容 OpenAI data[].embedding 与 Ollama embeddings[0] 双结构
        let openai = r#"{"data":[{"embedding":[0.1,0.2,0.3]}]}"#;
        assert_eq!(parse_embed_response(openai).unwrap(), vec![0.1f32, 0.2, 0.3]);
        let ollama = r#"{"embeddings":[[0.4,0.5,0.6]]}"#;
        assert_eq!(parse_embed_response(ollama).unwrap(), vec![0.4f32, 0.5, 0.6]);
        // 缺向量 → 报错
        assert!(parse_embed_response("{}").is_err());
        assert!(parse_embed_response(r#"{"data":[]}"#).is_err());
    }
}
