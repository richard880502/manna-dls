import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { SchoolClass, WsTemplateSummary } from "../../api/types";
import { TemplateSelect } from "../template/TemplateSelect";

/** The teacher's classes in use. A failed load counts as none: a class is optional context. */
export function useActiveClasses(): SchoolClass[] | null {
  const [classes, setClasses] = useState<SchoolClass[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .listClasses()
      .then((list) => alive && setClasses(list.filter((c) => c.archived_at === null)))
      .catch(() => alive && setClasses([]));
    return () => {
      alive = false;
    };
  }, []);
  return classes;
}

export const NO_CLASS_LABEL = "不指定";

/** 「班級：<name> ▾」: a pill holding a select. */
export function ClassChip({
  classes,
  value,
  disabled,
  onChange,
}: {
  classes: Array<Pick<SchoolClass, "id" | "name">>;
  value: number | null;
  disabled?: boolean;
  onChange: (classId: number | null) => void;
}) {
  return (
    <label className="prep-chip-select">
      <span>班級：</span>
      <select
        aria-label="班級"
        value={value === null ? "" : String(value)}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
      >
        <option value="">{NO_CLASS_LABEL}</option>
        {classes.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
    </label>
  );
}

/** 「照我的版型：<title> ▾」 */
export function TemplateChip({
  id,
  templates,
  value,
  disabled,
  onChange,
}: {
  id: string;
  templates: WsTemplateSummary[];
  value: number | null;
  disabled?: boolean;
  onChange: (templateId: number | null) => void;
}) {
  return (
    <label className="prep-chip-select" htmlFor={id}>
      <span>照我的版型：</span>
      <TemplateSelect id={id} templates={templates} value={value} disabled={disabled} onChange={onChange} />
    </label>
  );
}
