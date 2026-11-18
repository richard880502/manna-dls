import { useCallback, useState } from "react";
import { MaterialBrowser } from "../components/library/BrowsePanel";
import { UseMaterialMenu } from "../components/library/Cards";
import { PREVIEW, cardTitle, unusableReason, usableMaterial } from "../components/library/labels";
import { MaterialViewer } from "../components/material/MaterialViewer";

/**
 * 平台教材 (/reference): the knowledge base's materials (課綱, 詞彙表, past 學測／統測 papers…).
 * The material itself is read only; each card opens in the viewer (where the teacher's own notes,
 * which only she sees, work as on her own materials) and opens a tool with it (用這篇出題 ▾).
 */
export function ReferencePage() {
  const [viewing, setViewing] = useState<number | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);
  return (
    <section className="library-page" aria-labelledby="reference-heading">
      <div className="page-head">
        <h1 id="reference-heading">平台教材</h1>
        <p className="lead">Manna 知識庫的課綱、詞彙表和歷屆學測、統測英文試題。每位老師都看得到，可以直接拿來出題。</p>
      </div>
      <MaterialBrowser
        area="reference"
        empty={<p className="empty-line">知識庫還沒有教材。</p>}
        renderActions={(card) => {
          const title = cardTitle(card);
          return (
            <>
              <button
                type="button"
                className="ghost-button small-button"
                aria-label={`${PREVIEW}「${title}」`}
                aria-expanded={viewing === card.id}
                aria-controls="material-viewer"
                onClick={() => setViewing((cur) => (cur === card.id ? null : card.id))}
              >
                {PREVIEW}
              </button>
              <UseMaterialMenu card={card} disabled={!usableMaterial(card)} title={unusableReason(card)} />
            </>
          );
        }}
      />
      <MaterialViewer
        mode="panel"
        materialId={viewing}
        open={viewing !== null}
        onClose={closeViewer}
      />
    </section>
  );
}
