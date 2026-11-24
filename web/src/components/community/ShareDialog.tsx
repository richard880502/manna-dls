import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { CommunityMine, CommunityShareName, CommunityShareState } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { Dialog, DialogCancel } from "../Dialog";
import { ErrorText } from "../common";
import { ANONYMOUS_NAME } from "./labels";

/**
 * Sharing a worksheet is the teacher's choice and nothing is shared before this dialog is
 * answered: it says what others will see and what they never will, asks how the name is shown
 * (代號 or 匿名) and whether others may remix it, and says it can be withdrawn at any time.
 * Opened again for a shared worksheet it changes those two options.
 */
export function ShareDialog({
  item,
  onClose,
  onShared,
}: {
  item: CommunityMine;
  onClose: () => void;
  onShared: (state: CommunityShareState) => void;
}) {
  const { teacher } = useApp();
  const [name, setName] = useState<CommunityShareName>(item.name ?? "anonymous");
  const [allowRemix, setAllowRemix] = useState(item.shared ? item.allow_remix : true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      const state = await api.community.share(item.id, { shared: true, name, allow_remix: allowRemix });
      onShared(state);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title={item.shared ? "分享設定" : "分享這份學習單"} onClose={onClose} busy={busy}>
      <div className="com-share">
        <p>
          <strong>{item.title || "未命名的學習單"}</strong>
        </p>
        <div className="com-share-lists">
          <div>
            <h3>其他老師看得到</h3>
            <ul>
              <li>學習單上印出來的內容（學生版，沒有參考答案和教師備註）</li>
              <li>標題，和你選的名稱</li>
              <li>被收藏和被改編的次數</li>
            </ul>
          </div>
          <div>
            <h3>其他老師看不到</h3>
            <ul>
              <li>你的班級、課例、教材庫和其他作品</li>
              <li>你給 Manna 的要求和備課對話</li>
              <li>你選匿名時，你的研究代號</li>
            </ul>
          </div>
        </div>

        <fieldset className="com-share-field">
          <legend>名稱要怎麼顯示？</legend>
          <label className="check-row">
            <input type="radio" name="share-name" checked={name === "code"} onChange={() => setName("code")} />
            代號：顯示你的研究代號（{teacher?.research_id ?? ""}）
          </label>
          <label className="check-row">
            <input
              type="radio"
              name="share-name"
              checked={name === "anonymous"}
              onChange={() => setName("anonymous")}
            />
            匿名：顯示「{ANONYMOUS_NAME}」
          </label>
        </fieldset>

        <label className="check-row">
          <input type="checkbox" checked={allowRemix} onChange={(e) => setAllowRemix(e.target.checked)} />
          允許其他老師改編
        </label>
        {allowRemix && (
          <p className="field-hint">
            改編的老師會得到一份自己的副本，裡面有教材原文和參考答案，但沒有你的名字和你的其他資料。之後你取消分享，已經做好的副本會留在他們那裡。
          </p>
        )}
        <p className="field-hint">隨時都可以撤回：取消分享後，其他老師馬上就看不到了。</p>
        <ErrorText message={error} />
        <div className="button-row">
          <button type="button" className="primary-button" disabled={busy} onClick={() => void submit()}>
            {item.shared ? "儲存設定" : "分享"}
          </button>
          <DialogCancel disabled={busy} />
        </div>
      </div>
    </Dialog>
  );
}
