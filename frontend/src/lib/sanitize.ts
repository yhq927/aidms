import DOMPurify from "dompurify";

/**
 * 净化搜索高亮 HTML：仅放行 <mark> 标签与无属性，其余一律剥离（防 XSS）。
 * 检索结果片段中的关键词高亮由 Rust 端生成 <mark> 包裹，前端不可信内容经此净化后再注入。
 */
export function sanitizeHighlight(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: ["mark"],
    ALLOWED_ATTR: [],
  });
}
