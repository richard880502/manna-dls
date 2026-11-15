import { useState } from "react";
import { MaterialsSection } from "../components/material/MaterialLibrary";
import { OncePerPage } from "../components/OncePerPage";
import { TemplatesSection } from "../components/template/TemplatesSection";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";

/** 教材: the material library on its own page (the top bar's third destination), then 我的學習單版型. */
export function MaterialsPage() {
  const { teacher } = useApp();
  // A worksheet uploaded for a template also lands in the library above it.
  const [added, setAdded] = useState(0);
  if (!can(teacher, "use_materials")) {
    return (
      <div className="page-head">
        <h1>教材</h1>
        <p className="lead">你的帳號目前沒有使用教材庫的權限。</p>
      </div>
    );
  }
  return (
    <OncePerPage>
      <MaterialsSection asPage refreshSignal={added} />
      <TemplatesSection onMaterialAdded={() => setAdded((n) => n + 1)} />
    </OncePerPage>
  );
}
