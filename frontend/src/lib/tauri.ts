/**
 * Tauri invoke 统一封装（安全基线：唯一出网 / 唯一 IPC 入口）。
 *
 * 阶段 1 占位：仅在 Tauri 运行时 (`tauri:` 协议 / `__TAURI__`) 才允许调用后端命令；
 * 浏览器/Vite dev 下调用会显式报错，避免误把前端直连当成 IPC。
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export function isTauri(): boolean {
  if (typeof window === "undefined") return false;
  if (window.location?.protocol === "tauri:") return true;
  return "__TAURI__" in window;
}

/** 调用后端 Rust command。非 Tauri 环境抛出明确错误。 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error(`不在 Tauri 运行时，无法调用命令: ${cmd}`);
  }
  return tauriInvoke<T>(cmd, args);
}
