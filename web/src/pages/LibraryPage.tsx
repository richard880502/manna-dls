import { useEffect } from "react";
import { UnitsPanel } from "../components/UnitsPanel";
import { useApp } from "../context/AppContext";

/**
 * 備課庫: the teacher's own collection of ready-made unit designs (unit, pre-unit CoRe, lessons
 * and plans). One design, reusable across classes. Coming here leaves the class context: a unit
 * opened from the library has no current class unless only one class uses it.
 */
export function LibraryPage() {
  const { setCurrentClassId } = useApp();
  useEffect(() => {
    setCurrentClassId(null);
  }, [setCurrentClassId]);

  return (
    <section aria-labelledby="library-heading">
      <div className="page-intro">
        <p className="eyebrow warm">01 · Study &amp; Plan</p>
        <h2 id="library-heading">備課庫：你已經準備好的單元設計</h2>
        <p>
          每個單元設計包含單元前 CoRe、課例與教案，一份設計可以給好幾個班級用。先寫 CoRe 或先寫課例都可以，
          留白或略過 AI 都不會被擋下。
        </p>
      </div>
      <UnitsPanel />
    </section>
  );
}
