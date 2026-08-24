/**
 * 文件夹监控客户端：封装 start_folder_watch / stop_folder_watch / get_folder_watch_status。
 * mock 模式（浏览器预览）仅记录内存状态，便于 Settings 页演示；真机走 Tauri 命令。
 */
import { invoke, isTauri } from "./tauri";

export interface FolderWatchStatus {
  /** 是否监控中 */
  running: boolean;
  /** 监控目录 */
  path: string | null;
}

// mock 模式下的内存状态（同一会话内跨页面保持）
const mockState: FolderWatchStatus = { running: false, path: null };

/** 启动监控：path 目录 + defaultEntityIds 默认归属主体（可多选，空=未归类） */
export async function startFolderWatch(
  path: string,
  entityIds: number[]
): Promise<void> {
  if (!isTauri()) {
    mockState.running = true;
    mockState.path = path;
    return;
  }
  return invoke("start_folder_watch", {
    path,
    defaultEntityIds: entityIds,
  });
}

/** 停止监控 */
export async function stopFolderWatch(): Promise<void> {
  if (!isTauri()) {
    mockState.running = false;
    mockState.path = null;
    return;
  }
  return invoke("stop_folder_watch");
}

/** 获取当前监控状态 */
export async function getFolderWatchStatus(): Promise<FolderWatchStatus> {
  if (!isTauri()) {
    return { ...mockState };
  }
  return invoke<FolderWatchStatus>("get_folder_watch_status");
}
