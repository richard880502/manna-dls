import { MaterialsSection } from "../components/material/MaterialLibrary";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";

/** 教材: the material library on its own page (the top bar's third destination). */
export function MaterialsPage() {
  const { teacher } = useApp();
  if (!can(teacher, "use_materials")) {
    return (
      <div className="page-head">
        <h1>教材</h1>
        <p className="lead">你的帳號目前沒有使用教材庫的權限。</p>
      </div>
    );
  }
  return <MaterialsSection asPage />;
}
