import { useState, useEffect, useCallback } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { Upload, FileText, Download, Plus, Link2, Star, Trash2 } from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { useFilterStore } from "@/stores/useFilterStore";
import { useCatalogStore } from "@/stores/useCatalogStore";
import { useEntityNameMap } from "@/lib/catalog";
import { isTauri } from "@/lib/tauri";
import { importFiles } from "@/lib/importClient";
import { ThreeDimensionalFilter } from "@/components/ThreeDimensionalFilter";
import { AdvancedFilterPanel } from "@/components/AdvancedFilterPanel";
import { UnclassifiedBadge } from "@/components/UnclassifiedBadge";
import { EntityPicker } from "@/components/EntityPicker";
import { ExportDialog } from "@/components/ExportDialog";
import { BusinessForm } from "@/components/BusinessForm";
import { DocumentDrawer } from "@/components/DocumentDrawer";
import {
  listDocumentsWithEntities,
  deleteDocument,
  linkEntity,
  type DocumentWithEntities,
  type DocumentFilterInput,
} from "@/lib/documentClient";

/** 收藏本地存储键（R11：本地演示级，document 表无收藏列；mock/真机通用） */
const FAV_KEY = "aidms-favorites";

function loadFavs(): number[] {
  try {
    const raw = localStorage.getItem(FAV_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? arr.map(Number).filter((n) => Number.isFinite(n)) : [];
  } catch {
    return [];
  }
}

export default function Library() {
  const [docs, setDocs] = useState<DocumentWithEntities[]>([]);
  const [loading, setLoading] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [bizOpen, setBizOpen] = useState(false);
  const [drawerDoc, setDrawerDoc] = useState<DocumentWithEntities | null>(null);
  const [importing, setImporting] = useState(false);
  // P2-9：真机导入成功后弹主体多选器批量关联（存待关联的 doc id 列表）
  const [linkDocs, setLinkDocs] = useState<number[]>([]);
  // R11 收藏/最近：listView 切换；favorites 存 localStorage
  const [listView, setListView] = useState<"all" | "recent" | "fav">("all");
  const [favorites, setFavorites] = useState<number[]>(loadFavs);

  useEffect(() => {
    localStorage.setItem(FAV_KEY, JSON.stringify(favorites));
  }, [favorites]);

  function toggleFav(id: number) {
    setFavorites((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  }

  /** 删除资料（P2-1）：确认后调用后端级联删除，刷新列表 */
  async function handleDelete(d: DocumentWithEntities) {
    if (!window.confirm(`确定删除「${d.title}」？将同时清理索引、归属、标签、关联与自定义字段值。`)) return;
    try {
      await deleteDocument(d.id);
      toast.success(`已删除「${d.title}」`);
      if (drawerDoc?.id === d.id) setDrawerDoc(null);
      refresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  const entityId = useFilterStore((s) => s.entityId);
  const docType = useFilterStore((s) => s.docType);
  const tagId = useFilterStore((s) => s.tagId);
  const entityIds = useFilterStore((s) => s.entityIds);
  const dateFrom = useFilterStore((s) => s.dateFrom);
  const dateTo = useFilterStore((s) => s.dateTo);
  const source = useFilterStore((s) => s.source);
  const owner = useFilterStore((s) => s.owner);
  const unclassified = useFilterStore((s) => s.unclassified);
  const reset = useFilterStore((s) => s.reset);
  const entityNames = useEntityNameMap();
  const loadCatalog = useCatalogStore((s) => s.load);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listDocumentsWithEntities({
        // 未归类模式：后端无"entity_ids 为空"直接筛参，取全量后在列表端过滤（P2-2）
        entity_id: unclassified ? null : entityId,
        doc_type: docType,
        tag_id: tagId,
        entity_ids: unclassified ? [] : entityIds,
        date_from: dateFrom,
        date_to: dateTo,
        source,
        owner,
      });
      setDocs(unclassified ? list.filter((d) => d.entity_ids.length === 0) : list);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [entityId, docType, tagId, entityIds, dateFrom, dateTo, source, owner, unclassified]);

  useEffect(() => {
    loadCatalog();
    refresh();
  }, [loadCatalog, refresh]);

  /**
   * 导入主链路（R1 / P0-3）：
   * - 真机（Tauri）：原生 dialog 多选文件 → Rust `import_files`（读取→解析→入库）→ toast 汇总 + 刷新。
   * - 浏览器预览（mock）：保持既有演示逻辑（弹 EntityPicker）。
   */
  async function handleImport() {
    if (!isTauri()) {
      setPickerOpen(true);
      return;
    }
    let selected: string | string[] | null = null;
    try {
      selected = await open({
        multiple: true,
        directory: false,
        title: "选择要导入的资料文件",
        filters: [
          {
            name: "资料文件",
            extensions: [
              "txt", "md", "csv", "pdf", "docx", "xlsx",
              "png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff",
            ],
          },
        ],
      });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
      return;
    }
    if (!selected || (Array.isArray(selected) && selected.length === 0)) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setImporting(true);
    try {
      const results = await importFiles(paths);
      const ok = results.filter((r) => r.status === "ok").length;
      const pending = results.filter((r) => r.status === "ocr_pending").length;
      const failed = results.length - ok - pending;
      if (ok > 0) {
        toast.success(`成功导入 ${ok} 个文件${pending ? `，${pending} 个待 OCR` : ""}${failed ? `，${failed} 个失败` : ""}`);
      } else if (failed > 0) {
        toast.error(`${failed} 个文件导入失败`);
      } else if (pending > 0) {
        toast.info(`${pending} 个文件已标记待 OCR`);
      }
      // 失败明细（含原因）
      results
        .filter((r) => r.status !== "ok")
        .forEach((r) => {
          console.warn(`[import] ${r.fileName}: ${r.status} ${r.message ?? ""}`);
        });
      // P2-9：真机导入成功后弹主体多选器，批量关联成功导入的文件（待归类主体提示）
      const okDocs = results
        .filter((r) => r.status === "ok" && r.docId != null)
        .map((r) => r.docId as number);
      if (okDocs.length > 0) {
        setLinkDocs(okDocs);
      }
      refresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setImporting(false);
    }
  }

  // 当前视图展示列表：全部 / 最近（按 updated_at 倒序前 30）/ 收藏（localStorage）
  const shown =
    listView === "recent"
      ? [...docs].sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1)).slice(0, 30)
      : listView === "fav"
        ? docs.filter((d) => favorites.includes(d.id))
        : docs;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">资料库</h1>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => setExportOpen(true)}>
            <Download className="h-4 w-4" /> 导出
          </Button>
          <Button variant="outline" onClick={() => setBizOpen(true)}>
            <Plus className="h-4 w-4" /> 业务条目
          </Button>
          <Button id="library-import-btn" onClick={handleImport} disabled={importing}>
            <Upload className="h-4 w-4" /> {importing ? "导入中…" : "导入 / 新建"}
          </Button>
        </div>
      </div>

      {/* R11 收藏/最近（本地演示级） */}
      <div className="flex w-fit rounded-md border p-0.5">
        {(
          [
            { key: "all", label: "全部" },
            { key: "recent", label: "最近" },
            { key: "fav", label: "收藏" },
          ] as { key: "all" | "recent" | "fav"; label: string }[]
        ).map((t) => (
          <button
            key={t.key}
            onClick={() => setListView(t.key)}
            className={cn(
              "rounded px-3 py-1 text-sm transition-colors",
              listView === t.key
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            {t.label}
          </button>
        ))}
      </div>

      <ThreeDimensionalFilter />
      <AdvancedFilterPanel />

      <div className="flex justify-end">
        <Button variant="ghost" size="sm" onClick={() => reset()}>
          清空筛选
        </Button>
      </div>

      {loading && <p className="text-sm text-muted-foreground">加载中…</p>}

      <div className="space-y-3">
        {!loading && shown.length === 0 && (
          <Card>
            <CardContent className="text-muted-foreground">
              {listView === "fav"
                ? "暂无收藏。点击资料卡右上角星标即可收藏。"
                : listView === "recent"
                  ? "暂无资料。点击右上角「导入 / 新建」并选择归属主体。"
                  : "暂无资料。点击右上角「导入 / 新建」并选择归属主体。"}
            </CardContent>
          </Card>
        )}
        {shown.map((d) => {
          const fav = favorites.includes(d.id);
          return (
            <Card key={d.id}>
              <CardHeader className="pb-1">
                <div className="flex items-center justify-between gap-2">
                  <CardTitle className="flex items-center gap-2 text-base">
                    <FileText className="h-4 w-4 text-muted-foreground" />
                    {d.title}
                  </CardTitle>
                  <div className="flex shrink-0 items-center gap-1.5">
                    {d.doc_type && <Badge variant="outline">{d.doc_type}</Badge>}
                    {d.status === "ocr_pending" && (
                      <Badge variant="warning" title="扫描件待 OCR（feature=ocr 未启用或识别未完成）">
                        待 OCR
                      </Badge>
                    )}
                    {d.status === "parse_failed" && (
                      <Badge variant="error" title="解析失败，详情见内容">解析失败</Badge>
                    )}
                    {d.entity_ids.length === 0 ? (
                      <UnclassifiedBadge />
                    ) : (
                      d.entity_ids.slice(0, 3).map((id) => (
                        <Badge key={id} variant="secondary">
                          {entityNames.get(id) ?? `主体 #${id}`}
                        </Badge>
                      ))
                    )}
                    <button
                      type="button"
                      onClick={() => toggleFav(d.id)}
                      title={fav ? "取消收藏" : "收藏"}
                      className="rounded p-0.5 transition-colors hover:bg-accent"
                    >
                      <Star
                        className={cn(
                          "h-4 w-4",
                          fav ? "fill-warning text-warning" : "text-muted-foreground"
                        )}
                      />
                    </button>
                  </div>
                </div>
              </CardHeader>
              <CardContent>
                {d.content_text && (
                  <p className="line-clamp-2 text-sm text-muted-foreground">
                    {d.content_text}
                  </p>
                )}
                <div className="mt-2 flex items-center justify-between">
                  <span className="text-xs text-muted-foreground">
                    {d.kind === "business" ? "业务条目" : "文件"} ·{" "}
                    {d.date_field ?? "无日期"} · 更新 {d.updated_at.slice(0, 10)}
                  </span>
                  <div className="flex items-center gap-1">
                    <Button variant="ghost" size="sm" onClick={() => setDrawerDoc(d)}>
                      <Link2 className="h-4 w-4" /> 关联
                    </Button>
                    {/* P2-1：删除资料（确认后级联清理索引与关联） */}
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-muted-foreground hover:text-destructive"
                      onClick={() => handleDelete(d)}
                    >
                      <Trash2 className="h-4 w-4" /> 删除
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      <EntityPicker
        open={pickerOpen}
        fileName="示例_导入文件.pdf"
        onCancel={() => setPickerOpen(false)}
        onConfirm={(ids) => {
          setPickerOpen(false);
          toast.success(
            ids.length
              ? `已选择 ${ids.length} 个归属主体（待入库）`
              : "留空：将标为未归类主体（待入库）"
          );
        }}
      />

      {/* P2-9：真机导入成功后批量关联主体（仅成功入库的文件） */}
      <EntityPicker
        open={linkDocs.length > 0}
        fileName={`已导入 ${linkDocs.length} 个文件（选择归属主体）`}
        onCancel={() => setLinkDocs([])}
        onConfirm={async (ids) => {
          const docs = linkDocs;
          setLinkDocs([]);
          if (ids.length === 0) {
            toast.success("已标为未归类主体（可在详情中补归类）");
            return;
          }
          try {
            for (const docId of docs) {
              for (const eid of ids) await linkEntity(docId, eid);
            }
            toast.success(`已关联 ${docs.length} 个文件到 ${ids.length} 个主体`);
          } catch (e) {
            toast.error(e instanceof Error ? e.message : String(e));
          }
          refresh();
        }}
      />

      <ExportDialog
        open={exportOpen}
        onClose={() => setExportOpen(false)}
        filter={{
          entity_id: entityId,
          doc_type: docType,
          tag_id: tagId,
          entity_ids: entityIds,
          date_from: dateFrom,
          date_to: dateTo,
          source,
          owner,
        } satisfies DocumentFilterInput}
      />

      <BusinessForm
        open={bizOpen}
        onClose={() => setBizOpen(false)}
        onCreated={() => refresh()}
      />

      <DocumentDrawer
        open={!!drawerDoc}
        docId={drawerDoc?.id ?? 0}
        title={drawerDoc?.title ?? ""}
        docType={drawerDoc?.doc_type ?? null}
        contentText={drawerDoc?.content_text ?? null}
        entityIds={drawerDoc?.entity_ids ?? []}
        onEntityChange={() => refresh()}
        onClose={() => setDrawerDoc(null)}
      />
    </div>
  );
}
