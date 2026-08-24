/**
 * 录入 / 导入主体多选器（弹窗）：为文件或业务条目选择归属主体（可多选，不强制单一）。
 * R16 归属提示：基于文件名 / 文本关键词匹配已有主体名称给出「建议」，
 *   仅提示、不自动勾选、不自动去重，由用户最终判定（避免误删 / 误归）。
 */
import { useEffect, useMemo, useState } from "react";
import { X, Lightbulb } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useCatalogStore } from "@/stores/useCatalogStore";
import { UNCLASSIFIED_LABEL } from "@/lib/docTypes";

interface EntityPickerProps {
  open: boolean;
  /** 文件名（拖拽 / 选择导入时用于 R16 关键词匹配） */
  fileName?: string;
  /** 正文文本（可选，用于更细的归属提示） */
  text?: string;
  /** 已选主体 id（编辑场景回填） */
  initial?: number[];
  onConfirm: (ids: number[]) => void;
  onCancel: () => void;
}

/** 命中规则：主体名称整体或去除常见后缀后，作为子串出现在文件名 / 文本中 */
function matchScore(name: string, haystack: string): boolean {
  if (!haystack) return false;
  const h = haystack.toLowerCase();
  const base = name.toLowerCase().replace(/(科技有限公司|有限公司|股份公司|集团|公司)$/u, "");
  if (name.toLowerCase().includes(h) && h.length >= 2) return true; // 文本含主体全称
  return base.length >= 2 && h.includes(base); // 文件名含主体简称
}

export function EntityPicker({
  open,
  fileName,
  text,
  initial = [],
  onConfirm,
  onCancel,
}: EntityPickerProps) {
  const entities = useCatalogStore((s) => s.entities);
  const [selected, setSelected] = useState<Set<number>>(new Set(initial));
  const [keyword, setKeyword] = useState("");

  useEffect(() => {
    if (open) setSelected(new Set(initial));
  }, [open, initial]);

  const haystack = [fileName ?? "", text ?? ""].join("\n");

  // R16 建议（仅提示，不自动勾选）
  const suggested = useMemo(() => {
    const set = new Set<number>();
    for (const e of entities) if (matchScore(e.name, haystack)) set.add(e.id);
    return set;
  }, [entities, haystack]);

  const filtered = entities.filter(
    (e) => !keyword || e.name.toLowerCase().includes(keyword.toLowerCase())
  );

  if (!open) return null;

  const toggle = (id: number) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-md rounded-lg border bg-card shadow-md">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h2 className="text-base font-semibold">选择归属主体</h2>
          <button onClick={onCancel} className="text-muted-foreground hover:text-foreground">
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-3 px-4 py-3">
          {fileName && (
            <p className="text-xs text-muted-foreground">
              文件名：<span className="font-medium text-foreground">{fileName}</span>
            </p>
          )}
          {/* R16 归属提示 */}
          {suggested.size > 0 && (
            <div className="flex items-start gap-2 rounded-md bg-warning/15 p-2 text-xs">
              <Lightbulb className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warning-foreground" />
              <span className="text-foreground">
                根据文件名 / 内容，建议归属：
                {[...suggested].map((id) => {
                  const e = entities.find((x) => x.id === id);
                  return e ? (
                    <span key={id} className="mx-0.5 inline-block rounded bg-warning/30 px-1.5 py-0.5">
                      {e.name}
                    </span>
                  ) : null;
                })}
                （仅提示，请自行确认，系统不做自动判定）
              </span>
            </div>
          )}

          <input
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            placeholder="搜索主体…"
            className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />

          <div className="max-h-60 space-y-1 overflow-y-auto">
            {filtered.map((e) => {
              const isSel = selected.has(e.id);
              const isSug = suggested.has(e.id);
              return (
                <button
                  key={e.id}
                  onClick={() => toggle(e.id)}
                  className={cn(
                    "flex w-full items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors",
                    isSel
                      ? "border-transparent bg-primary/10 text-foreground"
                      : "border-input bg-background hover:bg-accent"
                  )}
                >
                  <span className="truncate">{e.name}</span>
                  <span className="flex items-center gap-1.5">
                    {isSug && <Badge variant="warning">建议</Badge>}
                    {isSel && <Badge variant="info">已选</Badge>}
                  </span>
                </button>
              );
            })}
            {filtered.length === 0 && (
              <p className="px-1 py-2 text-xs text-muted-foreground">无匹配主体</p>
            )}
          </div>

          <p className="text-xs text-muted-foreground">
            留空 = 标为「{UNCLASSIFIED_LABEL}」（可后续在主体管理中补归类）
          </p>
        </div>

        <div className="flex justify-end gap-2 border-t px-4 py-3">
          <Button variant="ghost" onClick={onCancel}>
            取消
          </Button>
          <Button onClick={() => onConfirm([...selected])}>
            确认（{selected.size} 个主体）
          </Button>
        </div>
      </div>
    </div>
  );
}
