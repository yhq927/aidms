import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Plus, Pencil, Trash2 } from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useCatalogStore } from "@/stores/useCatalogStore";
import {
  createEntity,
  updateEntity,
  deleteEntity,
  type EntityRow,
  type EntityInput,
} from "@/lib/entityClient";

interface Draft {
  id: number | null;
  name: string;
  credit_code: string;
  note: string;
}

const EMPTY: Draft = { id: null, name: "", credit_code: "", note: "" };

export default function Entities() {
  const entities = useCatalogStore((s) => s.entities);
  const loadCatalog = useCatalogStore((s) => s.load);
  const addLocal = useCatalogStore((s) => s.addEntityLocal);
  const updateLocal = useCatalogStore((s) => s.updateEntityLocal);
  const removeLocal = useCatalogStore((s) => s.removeEntityLocal);

  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    loadCatalog();
  }, [loadCatalog]);

  async function save() {
    const name = draft.name.trim();
    if (!name) {
      toast.error("主体名称不能为空");
      return;
    }
    const input: EntityInput = {
      name,
      credit_code: draft.credit_code.trim() || null,
      note: draft.note.trim() || null,
    };
    try {
      if (draft.id == null) {
        const id = await createEntity(input);
        addLocal({
          id,
          name: input.name,
          credit_code: input.credit_code ?? null,
          note: input.note ?? null,
          created_at: new Date().toISOString(),
        });
        toast.success(`已新增主体：${name}`);
      } else {
        await updateEntity(draft.id, input);
        updateLocal(draft.id, { name, credit_code: input.credit_code, note: input.note });
        toast.success(`已更新主体：${name}`);
      }
      setDraft(EMPTY);
      setEditing(false);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  async function remove(e: EntityRow) {
    try {
      await deleteEntity(e.id);
      removeLocal(e.id);
      toast.success(`已删除主体：${e.name}`);
    } catch (err) {
      // 删除有归属资料的主体：后端拦截，明确提示
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(msg || "删除失败");
    }
  }

  function startEdit(e: EntityRow) {
    setDraft({
      id: e.id,
      name: e.name,
      credit_code: e.credit_code ?? "",
      note: e.note ?? "",
    });
    setEditing(true);
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">主体管理</h1>
        <Badge variant="secondary">典型 ≤ 5（软目标，不设硬上限）</Badge>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">
            {editing ? "编辑主体" : "新增主体"}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder="主体名称（如：重庆智习室科技有限公司）"
            className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />
          <div className="flex flex-wrap gap-2">
            <input
              value={draft.credit_code}
              onChange={(e) => setDraft({ ...draft, credit_code: e.target.value })}
              placeholder="统一社会信用代码（可选）"
              className="flex-1 rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            />
            <input
              value={draft.note}
              onChange={(e) => setDraft({ ...draft, note: e.target.value })}
              placeholder="备注（可选）"
              className="flex-1 rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            />
          </div>
          <div className="flex gap-2">
            <Button onClick={save}>
              <Plus className="h-4 w-4" /> {editing ? "保存修改" : "新增"}
            </Button>
            {editing && (
              <Button
                variant="ghost"
                onClick={() => {
                  setDraft(EMPTY);
                  setEditing(false);
                }}
              >
                取消
              </Button>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="space-y-2">
        {entities.length === 0 && (
          <p className="text-sm text-muted-foreground">暂无主体，请先新增。</p>
        )}
        {entities.map((e) => (
          <Card key={e.id}>
            <CardHeader className="pb-1">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-base">{e.name}</CardTitle>
                <div className="flex shrink-0 items-center gap-1.5">
                  <Button variant="ghost" size="icon" onClick={() => startEdit(e)} title="编辑">
                    <Pencil className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => remove(e)}
                    title="删除（有归属资料时将被拦截）"
                  >
                    <Trash2 className="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              </div>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              {e.credit_code && <span>信用代码：{e.credit_code}　</span>}
              {e.note && <span>备注：{e.note}　</span>}
              <span>创建：{e.created_at.slice(0, 10)}</span>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}
