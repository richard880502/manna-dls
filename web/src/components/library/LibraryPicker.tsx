import { useEffect, useId, useRef, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type {
  CommunityCard,
  MaterialCard as MaterialCardData,
  TemplateCard as TemplateCardData,
} from "../../api/types";
import { Dialog } from "../Dialog";
import { Cover } from "../community/Cover";
import type { PickedMaterial } from "../material/MaterialChooser";
import { MaterialContent } from "../material/MaterialViewer";
import { MaterialBrowser, SearchField, TemplateBrowser } from "./BrowsePanel";
import { Tabs } from "./Tabs";
import { useCommunityList } from "../community/useCommunityList";
import { useSearchText } from "./useBrowse";
import {
  CHOOSE,
  MATERIAL_AREA_LABEL,
  PICK_MATERIAL,
  PICK_TEMPLATE,
  PREVIEW,
  TEMPLATE_AREA_LABEL,
  VIEW_SOURCE,
  VIEW_TEMPLATE,
  cardTitle,
  templateCardTitle,
  unusableReason,
  usableMaterial,
  type MaterialArea,
  type TemplateArea,
} from "./labels";

/** A chosen card (or a material read by id) as the article slot keeps it. */
export const pickedOf = (m: {
  id: number;
  title: string;
  looks_like_worksheet?: boolean;
  paragraph_count?: number;
}): PickedMaterial => ({
  id: m.id,
  title: m.title,
  ...(m.looks_like_worksheet ? { looks_like_worksheet: true } : {}),
  ...(typeof m.paragraph_count === "number" ? { paragraph_count: m.paragraph_count } : {}),
});

type PickerProps =
  | {
      kind: "material";
      onPickMaterial: (card: MaterialCardData) => void;
      returnFocus?: () => HTMLElement | null;
      onClose: () => void;
    }
  | {
      kind: "template";
      onPickTemplate: (card: Pick<TemplateCardData, "id" | "title" | "section_count">) => void;
      returnFocus?: () => HTMLElement | null;
      onClose: () => void;
    };

/** What is being previewed inside the picker, and what choosing from the preview takes. */
interface Preview {
  materialId: number;
  title: string;
  /** null: nothing to choose from here (no text, or a template that is not ready). */
  choose: (() => void) | null;
  /** The button that opened it: focus goes back there. */
  from: HTMLElement | null;
}

/** One material read inside the picker (the viewer's sheet, without notes), with 選用. */
function PickerPreview({ preview, onBack }: { preview: Preview; onBack: () => void }) {
  const backRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    backRef.current?.focus();
  }, []);
  return (
    <section className="picker-preview" aria-label={`${PREVIEW}「${preview.title}」`}>
      <div className="picker-preview-bar">
        <button ref={backRef} type="button" className="text-link is-plain" onClick={onBack}>
          回到列表
        </button>
        {preview.choose && (
          <button type="button" className="primary-button small-button" onClick={preview.choose}>
            {CHOOSE}
          </button>
        )}
      </div>
      <MaterialContent materialId={preview.materialId} annotate={false} />
    </section>
  );
}

const MATERIAL_TABS: ReadonlyArray<readonly [MaterialArea, string]> = [
  ["reference", MATERIAL_AREA_LABEL.reference],
  ["mine", MATERIAL_AREA_LABEL.mine],
];
const TEMPLATE_TABS: ReadonlyArray<readonly [TemplateArea, string]> = [
  ["platform", TEMPLATE_AREA_LABEL.platform],
  ["mine", TEMPLATE_AREA_LABEL.mine],
  ["saved", TEMPLATE_AREA_LABEL.saved],
];

function SavedTemplateBrowser({
  onPick,
}: {
  onPick: (card: Pick<TemplateCardData, "id" | "title" | "section_count">) => void;
}) {
  const search = useSearchText();
  const list = useCommunityList("saved", { q: search.q || undefined });
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const choose = async (card: CommunityCard) => {
    if (busyId !== null) return;
    setBusyId(card.id);
    setError(null);
    try {
      onPick(await api.community.useAsTemplate(card.id));
    } catch (err) {
      setError(errorMessage(err));
      setBusyId(null);
    }
  };

  return (
    <div className="com-browse saved-template-browser">
      <SearchField label="搜尋我的收藏" placeholder="用標題搜尋" search={search} />
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {list.error && (
        <div className="lib-error">
          <p className="error-text" role="alert">
            {list.error}
          </p>
          <button type="button" className="text-link is-plain" onClick={list.retry}>
            再試一次
          </button>
        </div>
      )}
      {list.items === null && !list.error && <p role="status">載入中…</p>}
      {list.items !== null && list.items.length === 0 && !list.error && (
        <div className="com-empty">
          <p>
            {search.q
              ? "沒有符合的收藏。換個搜尋字看看。"
              : "還沒有收藏。在「社群」的「探索」看到喜歡的學習單，按「收藏」就會放在這裡。"}
          </p>
        </div>
      )}
      {list.unavailable > 0 && (
        <p className="field-hint">有 {list.unavailable} 份收藏的學習單已經取消分享，不會列在這裡。</p>
      )}
      {list.items !== null && list.items.length > 0 && (
        <ul className="com-grid" aria-label="可當範本的收藏">
          {list.items.map((card) => (
            <li key={card.id} className="com-card">
              <div className="com-cover-wrap">
                <div className="com-cover-btn" aria-hidden="true">
                  <Cover sheet={card.cover} tint={card.id} />
                </div>
              </div>
              <h3 className="com-title" title={card.title}>
                {card.title || "未命名的學習單"}
              </h3>
              <p className="com-by">
                <span className="com-owner">{card.owner_name}</span>
              </p>
              <button
                type="button"
                className="primary-button small-button"
                aria-label={`${CHOOSE}「${card.title || "未命名的學習單"}」`}
                disabled={busyId !== null}
                onClick={() => void choose(card)}
              >
                {busyId === card.id ? "建立範本中…" : CHOOSE}
              </button>
            </li>
          ))}
        </ul>
      )}
      {list.items !== null && list.items.length < list.total && (
        <p className="lib-more">
          <button type="button" className="secondary-button" disabled={list.loadingMore} onClick={() => void list.loadMore()}>
            {list.loadingMore ? "載入中…" : "顯示更多"}
          </button>
        </p>
      )}
    </div>
  );
}

/**
 * 從教材挑選 (平台教材 | 我的教材) or 從範本挑選 (平台範本 | 我的範本 | 我的收藏): the browse UI in a large
 * dialog (the whole screen on a phone). Choosing a card closes it; 預覽 and 看原檔 read the
 * material here, and Escape there goes back to the list (otherwise it closes the dialog).
 */
export function LibraryPicker(props: PickerProps) {
  const ids = useId();
  const [materialTab, setMaterialTab] = useState<MaterialArea>("reference");
  const [templateTab, setTemplateTab] = useState<TemplateArea>("platform");
  const [preview, setPreview] = useState<Preview | null>(null);
  const panelId = `${ids}-panel`;

  const back = () => {
    const from = preview?.from;
    setPreview(null);
    // The card's button is visible again once the list is back.
    setTimeout(() => from?.isConnected && from.focus(), 0);
  };

  const open = (next: Omit<Preview, "from">) =>
    setPreview({ ...next, from: document.activeElement instanceof HTMLElement ? document.activeElement : null });

  let body;
  if (props.kind === "material") {
    const pick = props.onPickMaterial;
    body = (
      <>
        <Tabs label="教材從哪裡來" tabs={MATERIAL_TABS} current={materialTab} panelId={panelId} onChange={setMaterialTab} />
        <div id={panelId} role="tabpanel" className="picker-panel" hidden={preview !== null}>
          <MaterialBrowser
            key={materialTab}
            area={materialTab}
            empty={
              <p className="empty-line">
                {materialTab === "reference"
                  ? "知識庫還沒有教材。"
                  : "我的教材還沒有教材。可以用「上傳檔案」或「貼上文字」附上一篇。"}
              </p>
            }
            renderActions={(card) => {
              const title = cardTitle(card);
              const usable = usableMaterial(card);
              return (
                <>
                  <button
                    type="button"
                    className="ghost-button small-button"
                    aria-label={`${PREVIEW}「${title}」`}
                    onClick={() => open({ materialId: card.id, title, choose: usable ? () => pick(card) : null })}
                  >
                    {PREVIEW}
                  </button>
                  <button
                    type="button"
                    className="primary-button small-button"
                    aria-label={`${CHOOSE}「${title}」`}
                    title={unusableReason(card)}
                    disabled={!usable}
                    onClick={() => pick(card)}
                  >
                    {CHOOSE}
                  </button>
                </>
              );
            }}
          />
        </div>
      </>
    );
  } else {
    const pick = props.onPickTemplate;
    body = (
      <>
        <Tabs label="範本從哪裡來" tabs={TEMPLATE_TABS} current={templateTab} panelId={panelId} onChange={setTemplateTab} />
        <div id={panelId} role="tabpanel" className="picker-panel" hidden={preview !== null}>
          {templateTab === "saved" ? (
            <SavedTemplateBrowser onPick={pick} />
          ) : (
            <TemplateBrowser
              key={templateTab}
              area={templateTab}
              empty={
                <p className="empty-line">
                  {templateTab === "platform"
                    ? "還沒有平台範本。"
                    : "還沒有我的範本。到「學習單範本」上傳一份你做過的學習單，或用「上傳檔案」。"}
                </p>
              }
              renderActions={(card) => {
                const title = templateCardTitle(card);
                const ready = card.status === "ready";
                const source = card.source_material;
                return (
                  <>
                    <Link
                      className="text-link lib-action-link"
                      to={`/templates/${card.id}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      aria-label={`${VIEW_TEMPLATE}「${title}」`}
                    >
                      {VIEW_TEMPLATE}
                    </Link>
                    {source && (
                      <button
                        type="button"
                        className="ghost-button small-button"
                        aria-label={`${VIEW_SOURCE}「${title}」`}
                        onClick={() => open({ materialId: source.id, title, choose: ready ? () => pick(card) : null })}
                      >
                        {VIEW_SOURCE}
                      </button>
                    )}
                    <button
                      type="button"
                      className="primary-button small-button"
                      aria-label={`${CHOOSE}「${title}」`}
                      disabled={!ready}
                      onClick={() => pick(card)}
                    >
                      {CHOOSE}
                    </button>
                  </>
                );
              }}
            />
          )}
        </div>
      </>
    );
  }

  return (
    <Dialog
      title={props.kind === "material" ? PICK_MATERIAL : PICK_TEMPLATE}
      size="large"
      initialFocus='input[type="search"]'
      returnFocus={props.returnFocus}
      onEscape={preview ? back : undefined}
      onClose={props.onClose}
    >
      {body}
      {preview && <PickerPreview preview={preview} onBack={back} />}
    </Dialog>
  );
}
