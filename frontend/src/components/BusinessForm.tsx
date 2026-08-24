/**
 * 业务条目预置表单（R2）：基础字段落 document 主列、扩展字段走 field_value（R12）。
 * P1-A：提交改走 `submit_parsed`（kind=business，fields=JSON，content_text 含全部字段值），
 * 入库即建 FTS5/chunk/向量/RAG 上下文，提交后即可被搜索命中（替代旧 create_document 纯 INSERT）。
 * P2-4：每个自定义字段同时调 `set_field_value` 写 field_value 表（结构化统一存储，
 * 与后端 FTS5/向量重建联动）；fields JSON 仍保留（后端拼入 content 可检索）。
 * 阶段 7 收尾：支持勾选已入库文件建立关联（document_link，kind=业务关联）。
 */
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { X, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useCatalogStore } from "@/stores/useCatalogStore";
import {
  BIZ_TYPES,
  getFieldDefs,
  addFieldDef,
  removeFieldDef,
  setFieldValue,
  type FieldDef,
} from "@/lib/fieldClient";
import { submitParsed } from "@/lib/documentClient";
import { listAllDocs, createLink } from "@/lib/linkClient";

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated?: () => void;
}

export function BusinessForm({ open, onClose, onCreated }: Props) {
  const entities = useCatalogStore((s) => s.entities);
  const [bizType, setBizType] = useState(BIZ_TYPES[0]);
  const [title, setTitle] = useState("");
  const [party, setParty] = useState("");
  const [owner, setOwner] = useState("");
  const [dateField, setDateField] = useState("");
  const [note, setNote] = useState("");
  const [selEntities, setSelEntities] = useState<number[]>([]);
  const [files, setFiles] = useState<{ id: number; title: string }[]>([]);
  const [selFiles, setSelFiles] = useState<number[]>([]);
  const [defs, setDefs] = useState<FieldDef[]>([]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  // R12：新增自定义字段（字段名 + 类型）
  const [newFieldName, setNewFieldName] = useState("");
  const [newFieldType, setNewFieldType] = useState("text");

  useEffect(() => {
    if (open) getFieldDefs(bizType).then(setDefs).catch(() => setDefs([]));
  }, [open, bizType]);

  /** R12：添加自定义字段定义，成功后刷新字段列表 */
  async function addField() {
    const name = newFieldName.trim();
    if (!name) {
      toast.error("请填写字段名");
      return;
    }
    try {
      // field_key 自动生成（避免中文/空格作为键带来的 SQL/检索歧义）
      await addFieldDef(bizType, `custom_${Date.now()}`, name, newFieldType);
      setNewFieldName("");
      setDefs(await getFieldDefs(bizType));
      toast.success(`已添加自定义字段「${name}」`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  /** R12 补删（P2-8）：删除用户自定义字段定义（预置字段不可删），级联清值并刷新 */
  async function removeField(d: FieldDef) {
    if (d.is_preset) return;
    if (!window.confirm(`确定删除自定义字段「${d.field_label}」？将同时删除所有文档中该字段的值。`)) return;
    try {
      await removeFieldDef(d.id);
      setValues((v) => {
        const next = { ...v };
        delete next[d.field_key];
        return next;
      });
      setDefs(await getFieldDefs(bizType));
      toast.success(`已删除自定义字段「${d.field_label}」`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  // 打开时加载已入库文件（仅 file 类），供勾选关联
  useEffect(() => {
    if (open) {
      listAllDocs()
        .then((docs) =>
          setFiles(docs.filter((d) => d.kind === "file").map((d) => ({ id: d.id, title: d.title })))
        )
        .catch(() => setFiles([]));
    }
  }, [open]);

  if (!open) return null;

  function toggleEntity(id: number) {
    setSelEntities((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  }

  function toggleFile(id: number) {
    setSelFiles((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  }

  async function submit() {
    if (!title.trim()) {
      toast.error("请填写名称");
      return;
    }
    setBusy(true);
    try {
      // P1-A：走 submit_parsed 入库（建 FTS5/chunk/向量/RAG 上下文，非 create_document 纯 INSERT）
      // 自定义字段值收集为 fields JSON（结构化存储 + 后端拼入 content 可检索）
      const customFields: Record<string, string> = {};
      for (const d of defs) {
        const v = (values[d.field_key] ?? "").trim();
        if (v) customFields[d.field_key] = v;
      }
      const fieldsJson =
        Object.keys(customFields).length > 0 ? JSON.stringify(customFields) : null;
      // content_text：基础字段值拼入（仅 title/party/owner/date/note 时也能被搜索命中、RAG 上下文非空）
      const contentLines = [
        title.trim(),
        party.trim(),
        owner.trim(),
        dateField.trim(),
        note.trim(),
      ].filter(Boolean);
      const id = await submitParsed({
        title: title.trim(),
        content_text: contentLines.join("\n"),
        fields: fieldsJson,
        source: null,
        kind: "business",
        source_kind: "business",
        entity_ids: selEntities,
        doc_type: bizType,
        party: party.trim() || null,
        owner: owner.trim() || null,
        date_field: dateField || null,
        note: note.trim() || null,
      });
      // P2-4：每个自定义字段同时写 field_value 表（结构化统一存储；后端自动重建 FTS5，
      // 已配置嵌入时同步重建向量）。字段值写失败不阻断主创建（fields JSON 仍拼入 content 可检索），
      // 以 warning 提示用户，避免「文档已建但表单报错」的困惑。
      for (const [k, v] of Object.entries(customFields)) {
        try {
          await setFieldValue(id, k, v);
        } catch (e) {
          console.warn(`[BusinessForm] 字段 ${k} 写入 field_value 失败:`, e);
          toast.warning(`字段「${k}」值写入失败，但仍可通过全文检索`); 
        }
      }
      // 关联已勾选文件（业务条目 → 文件，kind=业务关联）
      for (const fid of selFiles) {
        await createLink(id, fid, "业务关联");
      }
      toast.success("已创建业务条目");
      // 重置
      setTitle(""); setParty(""); setOwner(""); setDateField(""); setNote("");
      setSelEntities([]); setSelFiles([]); setValues({});
      onCreated?.();
      onClose();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-lg border bg-card shadow-lg">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h2 className="text-base font-semibold">新建业务条目</h2>
          <button onClick={onClose} className="text-muted-foreground hover:text-foreground">
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="space-y-3 p-4">
          <div className="flex flex-wrap gap-2">
            {BIZ_TYPES.map((t) => (
              <button
                key={t}
                onClick={() => setBizType(t)}
                className={
                  "rounded-md border px-3 py-1 text-sm " +
                  (bizType === t ? "border-primary bg-accent text-accent-foreground" : "hover:bg-accent")
                }
              >
                {t}
              </button>
            ))}
          </div>

          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="名称（如：XX 采购合同）"
            className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />
          <div className="grid grid-cols-2 gap-2">
            <input value={party} onChange={(e) => setParty(e.target.value)} placeholder="相对方" className="rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />
            <input value={owner} onChange={(e) => setOwner(e.target.value)} placeholder="负责人" className="rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />
            <input value={dateField} onChange={(e) => setDateField(e.target.value)} placeholder="业务日期" className="rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />
            <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" className="rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />
          </div>

          {/* 动态自定义字段（R12） */}
          {defs.length > 0 && (
            <div className="space-y-2 rounded-md border p-2">
              <div className="text-xs text-muted-foreground">自定义字段（{bizType}）</div>
              {defs.map((d) => (
                <div key={d.id} className="flex items-center gap-2">
                  <label className="w-24 shrink-0 text-sm">{d.field_label}</label>
                  {d.field_type === "select" && d.options ? (
                    <select
                      value={values[d.field_key] ?? ""}
                      onChange={(e) => setValues((v) => ({ ...v, [d.field_key]: e.target.value }))}
                      className="flex-1 rounded-md border bg-background px-2 py-1.5 text-sm"
                    >
                      <option value="">—</option>
                      {JSON.parse(d.options).map((o: string) => (
                        <option key={o} value={o}>{o}</option>
                      ))}
                    </select>
                  ) : (
                    <input
                      type={d.field_type === "date" ? "date" : d.field_type === "number" ? "number" : "text"}
                      value={values[d.field_key] ?? ""}
                      onChange={(e) => setValues((v) => ({ ...v, [d.field_key]: e.target.value }))}
                      className="flex-1 rounded-md border bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    />
                  )}
                  {/* R12 补删（P2-8）：仅自定义字段（is_preset=0）可删除 */}
                  {!d.is_preset && (
                    <button
                      type="button"
                      onClick={() => removeField(d)}
                      title={`删除自定义字段「${d.field_label}」`}
                      className="shrink-0 rounded p-1 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* R12：添加自定义字段（get_field_defs / add_field_def 前端接线） */}
          <div className="space-y-2 rounded-md border border-dashed p-2">
            <div className="text-xs text-muted-foreground">添加自定义字段（{bizType}）</div>
            <div className="flex gap-2">
              <input
                value={newFieldName}
                onChange={(e) => setNewFieldName(e.target.value)}
                placeholder="字段名（如：结算方式）"
                className="flex-1 rounded-md border bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
              <select
                value={newFieldType}
                onChange={(e) => setNewFieldType(e.target.value)}
                className="rounded-md border bg-background px-2 py-1.5 text-sm"
              >
                <option value="text">文本</option>
                <option value="number">数字</option>
                <option value="date">日期</option>
                <option value="select">下拉</option>
              </select>
              <Button size="sm" variant="outline" onClick={addField}>
                <Plus className="h-4 w-4" /> 添加
              </Button>
            </div>
          </div>

          {/* 关联文件（阶段 7：业务条目 ↔ 已入库文件） */}
          <div className="space-y-2">
            <div className="text-xs text-muted-foreground">
              关联文件（可多选，勾选已入库文件建立关联）
            </div>
            <div className="flex flex-wrap gap-1.5">
              {files.length === 0 && (
                <span className="text-xs text-muted-foreground">暂无已入库文件</span>
              )}
              {files.map((f) => (
                <button
                  key={f.id}
                  onClick={() => toggleFile(f.id)}
                  className={cn(
                    "max-w-64 truncate rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                    selFiles.includes(f.id)
                      ? "border-primary bg-accent text-accent-foreground"
                      : "hover:bg-accent"
                  )}
                  title={f.title}
                >
                  {f.title}
                </button>
              ))}
            </div>
          </div>

          {/* 归属主体 */}
          <div className="space-y-2">
            <div className="text-xs text-muted-foreground">归属主体（可多选，留空=未归类）</div>
            <div className="flex flex-wrap gap-1.5">
              {entities.length === 0 && <span className="text-xs text-muted-foreground">暂无主体，请先在「主体管理」新增</span>}
              {entities.map((e) => (
                <button
                  key={e.id}
                  onClick={() => toggleEntity(e.id)}
                  className={
                    "rounded-full border px-2.5 py-0.5 text-xs " +
                    (selEntities.includes(e.id)
                      ? "border-primary bg-accent text-accent-foreground"
                      : "hover:bg-accent")
                  }
                >
                  {e.name}
                </button>
              ))}
            </div>
          </div>

          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onClose}>取消</Button>
            <Button onClick={submit} disabled={busy}>
              <Plus className="h-4 w-4" /> {busy ? "保存中…" : "创建"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
