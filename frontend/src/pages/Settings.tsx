/**
 * 配置页：LLM/嵌入配置（PRD §6.5.8 / 技术设计 §7）+ 文件夹监控（阶段 7 收尾）。
 *
 * LLM 配置：本地（Ollama）/ 云端（OpenAI 兼容）二选一；
 * 行内校验（base_url 协议、云端 Key 必填）；密钥仅经 IPC 传 Rust 存 OS keyring，不落前端。
 * 文件夹监控：选择目录 + 默认归属主体（多选）→ 启动/停止；新文件自动入库并归属默认主体。
 */
import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  FolderOpen,
  Play,
  Square,
  Sun,
  Moon,
  Save,
  Bot,
  KeyRound,
} from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";
import { useCatalogStore } from "@/stores/useCatalogStore";
import {
  startFolderWatch,
  stopFolderWatch,
  getFolderWatchStatus,
} from "@/lib/watchClient";
import {
  getLlmConfig,
  saveLlmConfig,
  getEmbedProbeStatus,
  type LlmConfig,
  type LlmProvider,
} from "@/lib/configClient";

const inputCls =
  "rounded-md border border-input bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";
const inputErrCls =
  "rounded-md border border-destructive bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-destructive";

export default function Settings() {
  const theme = useAppStore((s) => s.theme);
  const toggleTheme = useAppStore((s) => s.toggleTheme);

  const entities = useCatalogStore((s) => s.entities);
  const loadCatalog = useCatalogStore((s) => s.load);

  const [watchPath, setWatchPath] = useState("");
  const [watchEntities, setWatchEntities] = useState<number[]>([]);
  const [status, setStatus] = useState({ running: false, path: null as string | null });
  const [busy, setBusy] = useState(false);

  // ---- LLM 配置表单状态 ----
  const [llm, setLlm] = useState<LlmConfig | null>(null);
  const [provider, setProvider] = useState<LlmProvider>("ollama");
  const [baseUrl, setBaseUrl] = useState("");
  const [embedModel, setEmbedModel] = useState("");
  const [genModel, setGenModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [enabled, setEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [llmError, setLlmError] = useState<{ field: string; msg: string } | null>(null);
  // R5-P2-3：嵌入模型维度与内置 1024 维不符（语义检索降级关键词）提示
  const [embedDimMismatch, setEmbedDimMismatch] = useState(false);

  /** 读取嵌入维度探测状态并刷新提示（mismatch → 语义检索降级关键词） */
  function refreshEmbedProbeStatus() {
    getEmbedProbeStatus()
      .then((s) => setEmbedDimMismatch(s === "mismatch"))
      .catch(() => {});
  }

  useEffect(() => {
    loadCatalog();
    getFolderWatchStatus()
      .then((s) => setStatus({ running: s.running, path: s.path }))
      .catch(() => {});
    getLlmConfig()
      .then((c) => {
        setLlm(c);
        setProvider(c.provider ?? "ollama");
        setBaseUrl(c.base_url ?? "");
        setEmbedModel(c.embed_model ?? "");
        setGenModel(c.gen_model ?? "");
        setEnabled(c.enabled);
      })
      .catch(() => {});
    refreshEmbedProbeStatus();
  }, [loadCatalog]);

  // 校验失败后聚焦出错字段
  useEffect(() => {
    if (llmError) {
      const el = document.getElementById(`llm-${llmError.field}`);
      el?.focus();
    }
  }, [llmError]);

  function toggleEntity(id: number) {
    setWatchEntities((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id]
    );
  }

  async function doStart() {
    const p = watchPath.trim();
    if (!p) {
      toast.error("请填写要监控的目录路径");
      return;
    }
    setBusy(true);
    try {
      await startFolderWatch(p, watchEntities);
      setStatus({ running: true, path: p });
      toast.success("已启动文件夹监控");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function doStop() {
    setBusy(true);
    try {
      await stopFolderWatch();
      setStatus({ running: false, path: null });
      toast.success("已停止文件夹监控");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  // ---- LLM 配置：行内校验 + 保存 ----
  function validateLlm(): boolean {
    const url = baseUrl.trim();
    if (!/^https?:\/\//i.test(url)) {
      setLlmError({ field: "baseUrl", msg: "base_url 需以 http:// 或 https:// 开头" });
      return false;
    }
    if (provider === "openai_compat" && apiKey.trim() === "") {
      setLlmError({ field: "apiKey", msg: "云端模式需填写 API Key" });
      return false;
    }
    setLlmError(null);
    return true;
  }

  async function doSaveLlm() {
    if (!validateLlm()) return;
    setSaving(true);
    try {
      await saveLlmConfig({
        provider,
        baseUrl: baseUrl.trim(),
        embedModel: embedModel.trim() || null,
        genModel: genModel.trim() || null,
        // 云端才传 Key（本地 Ollama 无鉴权）；传空值视为不更新
        apiKey: provider === "openai_compat" ? apiKey.trim() || null : null,
        enabled,
      });
      const fresh = await getLlmConfig();
      setLlm(fresh);
      toast.success("AI 配置已保存");
      // R5-P2-3：保存后立即刷新维度探测状态（若该模型此前被探测为 mismatch 则提示）
      refreshEmbedProbeStatus();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  const llmStatusText = !llm
    ? "加载中…"
    : llm.enabled
      ? "已启用"
      : llm.base_url
        ? "已配置 · 未启用"
        : "未配置";

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">配置</h1>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Bot className="h-4 w-4" /> LLM / 嵌入配置
          </CardTitle>
          <CardContent className="space-y-3">
            <div className="flex items-center gap-2">
              <Badge variant={llm?.enabled ? "info" : "outline"}>{llmStatusText}</Badge>
              <span className="text-xs text-muted-foreground">
                密钥存 OS keychain（不落明文 / 不落日志）
              </span>
            </div>

            {/* 本地 / 云端 二选一 */}
            <div className="flex rounded-md border p-0.5">
              {(
                [
                  { key: "ollama", label: "本地（Ollama）" },
                  { key: "openai_compat", label: "云端（OpenAI 兼容）" },
                ] as { key: LlmProvider; label: string }[]
              ).map((p) => (
                <button
                  key={p.key}
                  type="button"
                  onClick={() => {
                    setProvider(p.key);
                    setLlmError(null);
                  }}
                  className={cn(
                    "rounded px-3 py-1 text-sm transition-colors",
                    provider === p.key
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                >
                  {p.label}
                </button>
              ))}
            </div>

            {/* R5-P2-3：嵌入模型维度与内置 1024 维不符 → 语义检索降级为关键词（不再静默） */}
            {embedDimMismatch && (
              <div className="rounded-md border border-warning bg-warning/10 px-3 py-2 text-sm text-warning-foreground">
                嵌入模型维度与内置 1024 维不符，语义检索已降级为关键词（向量补建已暂停）。
                如需恢复语义检索，请更换 1024 维嵌入模型后重新保存配置。
              </div>
            )}

            <div className="space-y-2">
              <div className="flex flex-col gap-1">
                <label htmlFor="llm-baseUrl" className="text-xs text-muted-foreground">
                  API base_url
                </label>
                <input
                  id="llm-baseUrl"
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  placeholder={
                    provider === "ollama"
                      ? "http://127.0.0.1:11434"
                      : "https://api.openai.com/v1"
                  }
                  className={cn(
                    llmError?.field === "baseUrl" ? inputErrCls : inputCls,
                    "min-w-64"
                  )}
                />
                {llmError?.field === "baseUrl" && (
                  <span className="text-xs text-destructive">{llmError.msg}</span>
                )}
              </div>

              {provider === "ollama" ? (
                <>
                  <div className="flex flex-col gap-1">
                    <label htmlFor="llm-embedModel" className="text-xs text-muted-foreground">
                      嵌入模型名
                    </label>
                    <input
                      id="llm-embedModel"
                      value={embedModel}
                      onChange={(e) => setEmbedModel(e.target.value)}
                      placeholder="如 nomic-embed-text / bge-m3"
                      className={cn(inputCls, "min-w-64")}
                    />
                  </div>
                  <div className="flex flex-col gap-1">
                    <label htmlFor="llm-genModel" className="text-xs text-muted-foreground">
                      生成模型名
                    </label>
                    <input
                      id="llm-genModel"
                      value={genModel}
                      onChange={(e) => setGenModel(e.target.value)}
                      placeholder="如 qwen2.5:7b"
                      className={cn(inputCls, "min-w-64")}
                    />
                  </div>
                </>
              ) : (
                <>
                  <div className="flex flex-col gap-1">
                    <label htmlFor="llm-apiKey" className="text-xs text-muted-foreground">
                      API Key{llm?.has_api_key ? "（已存，留空保持不变）" : ""}
                    </label>
                    <div className="flex items-center gap-1.5">
                      <KeyRound className="h-4 w-4 text-muted-foreground" />
                      <input
                        id="llm-apiKey"
                        type="password"
                        value={apiKey}
                        onChange={(e) => setApiKey(e.target.value)}
                        placeholder="sk-…"
                        className={cn(
                          llmError?.field === "apiKey" ? inputErrCls : inputCls,
                          "min-w-64 flex-1"
                        )}
                      />
                    </div>
                    {llmError?.field === "apiKey" && (
                      <span className="text-xs text-destructive">{llmError.msg}</span>
                    )}
                  </div>
                  <div className="flex flex-col gap-1">
                    <label htmlFor="llm-genModel" className="text-xs text-muted-foreground">
                      模型名（生成）
                    </label>
                    <input
                      id="llm-genModel"
                      value={genModel}
                      onChange={(e) => setGenModel(e.target.value)}
                      placeholder="如 gpt-4o-mini"
                      className={cn(inputCls, "min-w-64")}
                    />
                  </div>
                  <div className="flex flex-col gap-1">
                    <label htmlFor="llm-embedModel" className="text-xs text-muted-foreground">
                      嵌入模型名（可选）
                    </label>
                    <input
                      id="llm-embedModel"
                      value={embedModel}
                      onChange={(e) => setEmbedModel(e.target.value)}
                      placeholder="如 text-embedding-3-small"
                      className={cn(inputCls, "min-w-64")}
                    />
                  </div>
                </>
              )}

              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(e) => setEnabled(e.target.checked)}
                  className="h-4 w-4 rounded border-input"
                />
                启用 AI 能力（问答 / 语义检索）
              </label>

              <div className="flex items-center gap-2">
                <Button size="sm" onClick={doSaveLlm} disabled={saving}>
                  <Save className="h-4 w-4" /> {saving ? "保存中…" : "保存配置"}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={toggleTheme}
                >
                  {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
                  切换主题
                </Button>
              </div>
            </div>
          </CardContent>
        </CardHeader>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FolderOpen className="h-4 w-4" /> 文件夹监控
          </CardTitle>
          <CardContent className="space-y-3">
            <p className="text-sm text-muted-foreground">
              监控指定目录：新出现的文件自动尝试入库（与拖拽导入同一解析流水线），
              并默认归属下方选中的主体（留空=未归类）。
            </p>

            <div className="flex items-center gap-2">
              <Badge variant={status.running ? "info" : "outline"}>
                {status.running ? "监控中" : "未启动"}
              </Badge>
              {status.path && (
                <span className="truncate text-xs text-muted-foreground" title={status.path}>
                  {status.path}
                </span>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <input
                value={watchPath}
                onChange={(e) => setWatchPath(e.target.value)}
                placeholder="输入要监控的目录绝对路径"
                className={cn(inputCls, "min-w-64 flex-1")}
                disabled={status.running}
              />
              {!status.running ? (
                <Button size="sm" onClick={doStart} disabled={busy}>
                  <Play className="h-4 w-4" /> 启动
                </Button>
              ) : (
                <Button size="sm" variant="outline" onClick={doStop} disabled={busy}>
                  <Square className="h-4 w-4" /> 停止
                </Button>
              )}
            </div>

            <div className="space-y-1">
              <div className="text-xs text-muted-foreground">
                默认归属主体（可多选，新入库文件自动归属）
              </div>
              <div className="flex flex-wrap gap-1.5">
                {entities.length === 0 && (
                  <span className="text-xs text-muted-foreground">
                    暂无主体，请先在「主体管理」新增
                  </span>
                )}
                {entities.map((e) => (
                  <button
                    key={e.id}
                    type="button"
                    onClick={() => toggleEntity(e.id)}
                    className={cn(
                      "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                      watchEntities.includes(e.id)
                        ? "border-transparent bg-primary text-primary-foreground"
                        : "border-input bg-background text-foreground hover:bg-accent"
                    )}
                  >
                    {e.name}
                  </button>
                ))}
              </div>
            </div>
          </CardContent>
        </CardHeader>
      </Card>
    </div>
  );
}
