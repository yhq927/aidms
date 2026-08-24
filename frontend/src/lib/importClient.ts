/**
 * 文件导入客户端：封装 import_files 命令（R1 主链路 / P0-3）。
 *
 * 安全：前端只传文件路径清单，Rust 侧读取字节 → 解析（txt/csv/md/pdf/docx/xlsx）→ 入库；
 * 前端不读文件字节（安全约束禁止前端任意 fs 读，技术设计 §10）。
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 */
import { invoke, isTauri } from "./tauri";

export type ImportStatus = "ok" | "parse_failed" | "ocr_pending" | "error";

export interface ImportResult {
  path: string;
  fileName: string;
  status: ImportStatus;
  docId: number | null;
  title: string | null;
  message: string | null;
}

/** 调用后端导入流水线：多文件 → 逐文件解析入库，返回每文件结果 */
export async function importFiles(paths: string[]): Promise<ImportResult[]> {
  if (!isTauri()) return mockImport(paths);
  return invoke<ImportResult[]>("import_files", { paths });
}

/** 非 Tauri 环境下的演示结果（便于预览 toast 汇总） */
function mockImport(paths: string[]): ImportResult[] {
  return paths.map((p) => {
    const fileName = p.split(/[\\/]/).pop() ?? p;
    return {
      path: p,
      fileName,
      status: "ok" as const,
      docId: Math.floor(Math.random() * 10000) + 1,
      title: fileName,
      message: null,
    };
  });
}
