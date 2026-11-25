import { useRef } from "react";
import { Dialog } from "../../Dialog";
import { Loading, ErrorText } from "../../common";
import { VocabCheckPanel, useVocabCheck } from "../VocabCheckPanel";

/**
 * 詞彙檢查: the page `/worksheets/:id/check`, in a large dialog over the workspace. Whatever 處理
 * changed in the worksheet is read again by the workspace once the dialog closes (`onClose(changed)`).
 */
export function VocabCheckDialog({
  worksheetId,
  returnFocus,
  onClose,
}: {
  worksheetId: number;
  returnFocus: () => HTMLElement | null;
  onClose: (changed: boolean) => void;
}) {
  const changed = useRef(false);
  const ctl = useVocabCheck(worksheetId, () => {
    changed.current = true;
  });
  return (
    <Dialog title="詞彙級別檢查" size="large" returnFocus={returnFocus} onClose={() => onClose(changed.current)}>
      <div className="ws-check-dialog" lang="zh-Hant">
        <p className="lead">對照大考中心高中英文參考詞彙表，看這份學習單的學生版用了哪些級別的字。</p>
        {ctl.loadError ? <ErrorText message={ctl.loadError} /> : ctl.state ? <VocabCheckPanel id={worksheetId} ctl={ctl} inDialog /> : <Loading />}
      </div>
    </Dialog>
  );
}
