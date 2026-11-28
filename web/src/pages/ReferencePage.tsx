import { useCallback, useRef, useState } from "react";
import { adminLibraryApi } from "../api/client";
import type { MaterialCard as MaterialCardData } from "../api/types";
import { MaterialBrowser } from "../components/library/BrowsePanel";
import { MoreMenu, UseMaterialMenu } from "../components/library/Cards";
import { PREVIEW, cardTitle, unusableReason, usableMaterial } from "../components/library/labels";
import { PLATFORM_MATERIAL_DELETE_NOTE, PlatformDeleteDialog } from "../components/library/PlatformDeleteDialog";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";
import { useDocumentTitle } from "../lib/useDocumentTitle";

/**
 * 平台教材 (/reference): the knowledge base's materials (課綱, 詞彙表, past 學測／統測 papers…).
 * The material itself is read only; each card opens in the viewer (where the teacher's own notes,
 * which only she sees, work as on her own materials) and opens a tool with it (用這篇出題 ▾).
 * Accounts that manage the knowledge base (`manage_knowledge`) also delete one from its card's ⋯,
 * as in 教材與範本管理; other teachers see no ⋯.
 */
export function ReferencePage() {
  useDocumentTitle("平台教材");
  const { teacher } = useApp();
  const manages = can(teacher, "manage_knowledge");
  const [viewing, setViewing] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<MaterialCardData | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [changed, setChanged] = useState(0);
  const closeViewer = useCallback(() => setViewing(null), []);
  /** The card's ⋯: focus goes back there, or to the heading once the card is gone. */
  const opener = useRef<HTMLElement | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const backToOpener = useCallback(
    () => (opener.current?.isConnected ? opener.current : heading.current),
    [],
  );
  return (
    <section className="library-page" aria-labelledby="reference-heading">
      <div className="page-head">
        <h1 id="reference-heading" ref={heading} tabIndex={-1}>
          平台教材
        </h1>
        <p className="lead">Manna 知識庫的課綱、詞彙表和歷屆學測、統測英文試題。每位老師都看得到，可以直接拿來出題。</p>
      </div>
      {notice && (
        <p className="quiet-note" role="status">
          {notice}
        </p>
      )}
      <MaterialBrowser
        area="reference"
        refreshSignal={changed}
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
              {manages && (
                <MoreMenu
                  label={`更多動作「${title}」`}
                  items={[
                    {
                      label: "刪除",
                      onSelect: (from) => {
                        opener.current = from;
                        setDeleting(card);
                      },
                    },
                  ]}
                />
              )}
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
      {deleting && (
        <PlatformDeleteDialog
          title={cardTitle(deleting)}
          note={PLATFORM_MATERIAL_DELETE_NOTE}
          remove={() => adminLibraryApi.deleteMaterial(deleting.id)}
          returnFocus={backToOpener}
          onClose={() => setDeleting(null)}
          onDone={() => {
            if (viewing === deleting.id) setViewing(null);
            setDeleting(null);
            setNotice(`已永久刪除「${cardTitle(deleting)}」。`);
            setChanged((n) => n + 1);
          }}
        />
      )}
    </section>
  );
}
