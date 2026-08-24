/**
 * 资料导出对话框（R17）：按当前三维筛选导出 CSV / JSON，含主体标注。
 * 后端返回纯文本，前端负责生成 Blob 并触发下载。
 */
import { useState } from "react";
import { toast } from "sonner";
import { X, Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  exportDocuments,
  type ExportFormat,
  type DocumentFilterInput,
} from "@/lib/documentClient";

interface Props {
  open: boolean;
  onClose: () => void;
  filter: DocumentFilterInput;
}

export function ExportDialog({ open, onClose, filter }: Props) {
  const [format, setFormat] = useState<ExportFormat>("csv");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  async function doExport() {
    setBusy(true);
    try {
      const text = await exportDocuments(filter, format);
      const blob = new Blob([text], {
        type: format === "csv" ? "text/csv;charset=utf-8" : "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      const ts = new Date().toISOString().slice(0, 10);
      a.href = url;
      a.download = `aidms-export-${ts}.${format}`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast.success(`已导出 ${format.toUpperCase()}`);
      onClose();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-md rounded-lg border bg-card shadow-lg">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h2 className="text-base font-semibold">导出资料</h2>
          <button onClick={onClose} className="text-muted-foreground hover:text-foreground">
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="space-y-4 p-4">
          <p className="text-sm text-muted-foreground">
            将按当前筛选条件导出，字段含 <Badge variant="secondary">主体标注</Badge> 与标签。
          </p>
          <div className="flex gap-2">
            {(["csv", "json"] as ExportFormat[]).map((fmt) => (
              <button
                key={fmt}
                onClick={() => setFormat(fmt)}
                className={
                  "flex-1 rounded-md border px-3 py-2 text-sm capitalize " +
                  (format === fmt
                    ? "border-primary bg-accent text-accent-foreground"
                    : "hover:bg-accent")
                }
              >
                {fmt.toUpperCase()}
              </button>
            ))}
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onClose}>
              取消
            </Button>
            <Button onClick={doExport} disabled={busy}>
              <Download className="h-4 w-4" /> {busy ? "导出中…" : "下载"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
