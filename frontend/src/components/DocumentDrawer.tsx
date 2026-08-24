import { useEffect, useState } from "react";
import { X, Link2, Trash2, Plus, Tag } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { useCatalogStore } from "@/stores/useCatalogStore";
import { linkEntity, unlinkEntity } from "@/lib/documentClient";
import {
  listLinks,
  createLink,
  deleteLink,
  listAllDocs,
  type DocLink,
} from "@/lib/linkClient";
import {
  addDocumentTag,
  removeDocumentTag,
  listDocumentTags,
  createTag,
} from "@/lib/entityClient";

interface Props {
  open: boolean;
  docId: number;
  title: string;
  docType?: string | null;
  contentText?: string | null;
  entityIds?: number[];
  /** 归属变更（即点即存）后通知父级刷新列表（P2-3） */
  onEntityChange?: () => void;
  onClose: () => void;
}

export function DocumentDrawer({
  open,
  docId,
  title,
  docType,
  contentText,
  entityIds = [],
  onEntityChange,
  onClose,
}: Props) {
  const [links, setLinks] = useState<DocLink[]>([]);
  const [allDocs, setAllDocs] = useState<{ id: number; title: string }[]>([]);
  const [target, setTarget] = useState<number | "">("");
  const [kind, setKind] = useState("attachment");
  // 归属主体本地态：打开时同步自 props，点选即调后端（P2-3）
  const [curEntityIds, setCurEntityIds] = useState<number[]>(entityIds);
  const entities = useCatalogStore((s) => s.entities);
  const loadCatalog = useCatalogStore((s) => s.load);

  useEffect(() => {
    if (!open) return;
    loadCatalog();
    setCurEntityIds(entityIds);
    listAllDocs()
      .then((d) => setAllDocs(d.map((x) => ({ id: x.id, title: x.title }))))
      .catch(() => {});
    listLinks(docId).then(setLinks).catch(() => {});
  }, [open, docId, entityIds, loadCatalog]);

  const titleOf = (id: number) =>
    allDocs.find((d) => d.id === id)?.title ?? `#${id}`;

  // P2-3：标签打标本地态——打开时拉取当前文档已打标签
  const tags = useCatalogStore((s) => s.tags);
  const [curTagIds, setCurTagIds] = useState<number[]>([]);
  const [newTagName, setNewTagName] = useState("");
  const [busyTag, setBusyTag] = useState(false);
  useEffect(() => {
    if (!open) return;
    setCurTagIds([]);
    listDocumentTags(docId)
      .then(setCurTagIds)
      .catch(() => setCurTagIds([]));
  }, [open, docId]);

  /** 归属即点即存：点选/取消即调 linkEntity/unlinkEntity（P2-3） */
  async function toggleEntity(id: number) {
    const on = curEntityIds.includes(id);
    try {
      if (on) {
        await unlinkEntity(docId, id);
        setCurEntityIds((s) => s.filter((x) => x !== id));
      } else {
        await linkEntity(docId, id);
        setCurEntityIds((s) => [...s, id]);
      }
      onEntityChange?.();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  /** P2-3：标签点选即打标 / 即取消 */
  async function toggleTag(tagId: number) {
    const on = curTagIds.includes(tagId);
    try {
      if (on) {
        await removeDocumentTag(docId, tagId);
        setCurTagIds((s) => s.filter((x) => x !== tagId));
      } else {
        await addDocumentTag(docId, tagId);
        setCurTagIds((s) => [...s, tagId]);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  /** P2-3：创建新标签并立即打到本件（最小打标 UI 闭环） */
  async function addAndApplyTag() {
    const name = newTagName.trim();
    if (!name || busyTag) return;
    setBusyTag(true);
    try {
      const id = await createTag(name);
      await addDocumentTag(docId, id);
      // 刷新目录 store 标签列表 + 本件标签集
      useCatalogStore.getState().load();
      setCurTagIds((s) => [...s, id]);
      setNewTagName("");
      toast.success(`已创建标签「${name}」并打标`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyTag(false);
    }
  }

  const add = async () => {
    if (target === "" || target === docId) return;
    await createLink(docId, Number(target), kind);
    setLinks(await listLinks(docId));
    setTarget("");
  };

  // 方向感知删除：out（本件为 from）删 from_id=本件；in（本件为 to）删 from_id=对方
  const del = async (l: DocLink) => {
    if (l.direction === "out") await deleteLink(docId, l.id);
    else await deleteLink(l.id, docId);
    setLinks(await listLinks(docId));
  };

  // 关联目标候选：排除自身与已关联文档（避免重复建立）
  const linkedIds = new Set(links.map((l) => l.id));
  const candidates = allDocs.filter((d) => d.id !== docId && !linkedIds.has(d.id));

  if (!open) return null;

  return (
    <div className="fixed right-0 top-0 z-50 flex h-full w-[420px] flex-col border-l bg-card shadow-xl">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h2 className="text-base font-semibold">文档详情与关联</h2>
        <Button variant="ghost" size="icon" onClick={onClose}>
          <X className="h-4 w-4" />
        </Button>
      </div>
      <div className="flex-1 space-y-4 overflow-auto p-4">
        <div>
          <h3 className="text-lg font-medium">{title}</h3>
          <div className="mt-1 flex flex-wrap gap-1.5">
            {docType && <Badge variant="outline">{docType}</Badge>}
            {curEntityIds.length === 0 && <Badge variant="error">未归类主体</Badge>}
          </div>
          {/* 归属主体：点选即存（P2-3） */}
          <div className="mt-3 space-y-2 rounded-md border p-2">
            <div className="text-xs text-muted-foreground">归属主体（点击即存，可多选）</div>
            <div className="flex flex-wrap gap-1.5">
              {entities.length === 0 && (
                <span className="text-xs text-muted-foreground">暂无主体，请先在「主体管理」新增</span>
              )}
              {entities.map((e) => {
                const on = curEntityIds.includes(e.id);
                return (
                  <button
                    key={e.id}
                    type="button"
                    onClick={() => toggleEntity(e.id)}
                    className={cn(
                      "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                      on
                        ? "border-transparent bg-primary text-primary-foreground"
                        : "border-input bg-background text-foreground hover:bg-accent"
                    )}
                    title={on ? `取消「${e.name}」归属` : `归属到「${e.name}」`}
                  >
                    {e.name}
                  </button>
                );
              })}
            </div>
          </div>
          {/* P2-3：标签打标（点选即打/取消 + ＋ 新建并打标） */}
          <div className="mt-2 space-y-2 rounded-md border p-2">
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Tag className="h-3 w-3" /> 标签（点击即打标 / 取消）
            </div>
            <div className="flex flex-wrap gap-1.5">
              {tags.length === 0 && (
                <span className="text-xs text-muted-foreground">暂无标签，可在下方新建</span>
              )}
              {tags.map((t) => {
                const on = curTagIds.includes(t.id);
                return (
                  <button
                    key={t.id}
                    type="button"
                    onClick={() => toggleTag(t.id)}
                    className={cn(
                      "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                      on
                        ? "border-transparent bg-accent text-accent-foreground"
                        : "border-input bg-background text-foreground hover:bg-accent"
                    )}
                    title={on ? `取消「${t.name}」标签` : `打上「${t.name}」标签`}
                  >
                    {t.name}
                  </button>
                );
              })}
            </div>
            <div className="flex gap-1.5">
              <input
                value={newTagName}
                onChange={(e) => setNewTagName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && addAndApplyTag()}
                placeholder="＋ 新建标签并打标"
                className="flex-1 rounded-md border bg-background px-2 py-1 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
              <Button size="sm" variant="outline" onClick={addAndApplyTag} disabled={busyTag || !newTagName.trim()}>
                <Plus className="h-3 w-3" />
              </Button>
            </div>
          </div>
          {contentText && (
            <p className="mt-2 line-clamp-4 text-sm text-muted-foreground">
              {contentText}
            </p>
          )}
        </div>

        <div className="border-t pt-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium">
            <Link2 className="h-4 w-4" /> 关联文档（{links.length}）
          </div>
          <div className="space-y-2">
            {links.map((l) => (
              <div
                key={l.id + l.direction}
                className="flex items-center justify-between rounded-md border px-3 py-2 text-sm"
              >
                <div className="min-w-0">
                  <span className="font-medium">{titleOf(l.id)}</span>
                  <Badge variant="outline" className="ml-2">
                    {l.kind}
                  </Badge>
                  <span className="ml-2 text-xs text-muted-foreground">
                    {l.direction === "out" ? "→ 本件关联" : "← 关联本件"}
                  </span>
                </div>
                <Button variant="ghost" size="icon" onClick={() => del(l)}>
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
            {links.length === 0 && (
              <p className="text-sm text-muted-foreground">暂无关联</p>
            )}
          </div>

          <div className="mt-3 flex gap-2">
            <select
              className="flex-1 rounded-md border bg-background px-2 py-1.5 text-sm"
              value={target}
              onChange={(e) =>
                setTarget(e.target.value === "" ? "" : Number(e.target.value))
              }
            >
              <option value="">选择关联目标…</option>
              {candidates.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.title}
                </option>
              ))}
              {candidates.length === 0 && (
                <option value="" disabled>
                  暂无可关联文档
                </option>
              )}
            </select>
            <input
              className="w-28 rounded-md border bg-background px-2 py-1.5 text-sm"
              placeholder="类型"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
            />
            <Button size="sm" onClick={add}>
              <Plus className="h-4 w-4" /> 关联
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
