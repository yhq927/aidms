/**
 * 字段客户端（R2 预置表单 / R12 自定义字段）：封装 field_def / field_value 命令。
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 */
import { invoke, isTauri } from "./tauri";

export interface FieldDef {
  id: number;
  biz_type: string;
  field_key: string;
  field_label: string;
  field_type: string; // text/number/date/select
  options?: string | null; // select 选项 JSON
  is_preset: boolean;
}

export const BIZ_TYPES = ["客户", "合同", "项目", "供应商", "资质"];

/** 取某业务类型的字段定义（预置 + 用户自定义） */
export async function getFieldDefs(bizType: string): Promise<FieldDef[]> {
  if (!isTauri()) return mockDefs.filter((d) => d.biz_type === bizType);
  return invoke<FieldDef[]>("get_field_defs", { bizType });
}

/** 写入文档的自定义字段值（upsert，后端自动重建 FTS5） */
export async function setFieldValue(
  docId: number,
  fieldKey: string,
  value: string
): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("set_field_value", { docId, fieldKey, value });
}

/** 用户新增自定义字段定义 */
export async function addFieldDef(
  bizType: string,
  fieldKey: string,
  fieldLabel: string,
  fieldType: string
): Promise<number> {
  if (!isTauri()) return -1;
  return invoke<number>("add_field_def", {
    bizType,
    fieldKey,
    fieldLabel,
    fieldType,
  });
}

/** 删除用户自定义字段定义（P2-8：R12 补删；预置字段后端拒绝删除，幂等返回 0） */
export async function removeFieldDef(id: number): Promise<number> {
  if (!isTauri()) return 0;
  return invoke<number>("remove_field_def", { id });
}

// ---- 浏览器预览用 mock ----
const mockDefs: FieldDef[] = [
  { id: 1, biz_type: "合同", field_key: "amount", field_label: "合同金额", field_type: "number", is_preset: true },
  { id: 2, biz_type: "合同", field_key: "effective_date", field_label: "生效日期", field_type: "date", is_preset: true },
  { id: 3, biz_type: "合同", field_key: "expire_date", field_label: "到期日期", field_type: "date", is_preset: true },
  { id: 4, biz_type: "客户", field_key: "industry", field_label: "行业", field_type: "text", is_preset: true },
  { id: 5, biz_type: "客户", field_key: "contact", field_label: "联系人", field_type: "text", is_preset: true },
  { id: 6, biz_type: "项目", field_key: "budget", field_label: "预算", field_type: "number", is_preset: true },
];
