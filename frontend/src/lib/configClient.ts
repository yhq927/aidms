/**
 * LLM 配置客户端：封装 save_llm_config / get_llm_config / set_llm_enabled（技术设计 §7 / §10）。
 *
 * 敏感字段（api_key）仅经 IPC 传给 Rust 侧存 OS keyring，**不落前端存储/日志**；
 * 前端永不直接读取真实密钥（has_api_key 仅标记是否已存）。
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 */
import { invoke, isTauri } from "./tauri";

export type LlmProvider = "ollama" | "openai_compat";

export interface LlmConfig {
  provider: LlmProvider | null;
  base_url: string | null;
  embed_model: string | null;
  gen_model: string | null;
  enabled: boolean;
  /** 仅标记是否已存密钥引用，不含密钥本身 */
  has_api_key: boolean;
}

export interface SaveLlmConfigInput {
  provider: LlmProvider;
  baseUrl: string;
  embedModel?: string | null;
  genModel?: string | null;
  /** 仅本次传入，存 keyring，不落 DB */
  apiKey?: string | null;
  enabled: boolean;
}

/** 读取当前 LLM 配置（未配置时返回默认空配置） */
export async function getLlmConfig(): Promise<LlmConfig> {
  if (!isTauri()) return mockConfig;
  return invoke<LlmConfig>("get_llm_config");
}

/** 保存 LLM 配置（Rust 参数为结构体 input，键 camelCase，须整体包裹） */
export async function saveLlmConfig(cfg: SaveLlmConfigInput): Promise<void> {
  if (!isTauri()) {
    mockConfig = {
      provider: cfg.provider,
      base_url: cfg.baseUrl,
      embed_model: cfg.embedModel ?? null,
      gen_model: cfg.genModel ?? null,
      enabled: cfg.enabled,
      has_api_key: !!cfg.apiKey,
    };
    return;
  }
  return invoke<void>("save_llm_config", {
    input: {
      provider: cfg.provider,
      baseUrl: cfg.baseUrl,
      embedModel: cfg.embedModel ?? null,
      genModel: cfg.genModel ?? null,
      apiKey: cfg.apiKey ?? null,
      enabled: cfg.enabled,
    },
  });
}

/** 仅切换启用状态（问答/语义检索门控） */
export async function setLlmEnabled(enabled: boolean): Promise<void> {
  if (!isTauri()) {
    mockConfig = { ...mockConfig, enabled };
    return;
  }
  return invoke<void>("set_llm_enabled", { enabled });
}

/**
 * 读取嵌入维度探测状态（R5-P2-3）：返回 "ok" | "mismatch" | null（未配置/未探测）。
 * mismatch 表示嵌入模型维度与内置 1024 维不符，语义检索已降级为关键词，
 * 配置页据此展示提示（轻量命令，仅读 app_meta，不触发重建/探测）。
 */
export async function getEmbedProbeStatus(): Promise<string | null> {
  if (!isTauri()) return null;
  return invoke<string | null>("get_embed_probe_status");
}

// ---- 浏览器预览用 mock ----
let mockConfig: LlmConfig = {
  provider: null,
  base_url: null,
  embed_model: null,
  gen_model: null,
  enabled: false,
  has_api_key: false,
};
