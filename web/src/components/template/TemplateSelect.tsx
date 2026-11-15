import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { WsTemplateSummary } from "../../api/types";
import { NO_TEMPLATE_LABEL, templateTitle } from "../../lib/templates";

/**
 * The teacher's templates that can be used now (status ready). null while loading; a failed load
 * counts as none, since choosing a template is always optional.
 */
export function useReadyTemplates(): WsTemplateSummary[] | null {
  const [templates, setTemplates] = useState<WsTemplateSummary[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .listWorksheetTemplates()
      .then((res) => alive && setTemplates(res.templates.filter((t) => t.status === "ready")))
      .catch(() => alive && setTemplates([]));
    return () => {
      alive = false;
    };
  }, []);
  return templates;
}

/** 「照我的版型」: 不使用版型 or one of the ready templates. */
export function TemplateSelect({
  id,
  templates,
  value,
  disabled,
  describedBy,
  onChange,
}: {
  id: string;
  templates: WsTemplateSummary[];
  value: number | null;
  disabled?: boolean;
  describedBy?: string;
  onChange: (templateId: number | null) => void;
}) {
  return (
    <select
      id={id}
      value={value === null ? "" : String(value)}
      disabled={disabled}
      aria-describedby={describedBy}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
    >
      <option value="">{NO_TEMPLATE_LABEL}</option>
      {templates.map((t) => (
        <option key={t.id} value={t.id}>
          {templateTitle(t)}
          {t.version_no ? `（第 ${t.version_no} 版）` : ""}
        </option>
      ))}
    </select>
  );
}
