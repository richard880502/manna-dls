import type { WsGloss } from "../../api/types";

type GlossLine = Pick<WsGloss, "word" | "pos" | "meaning_zh" | "definition_en">;

/** 「(v.) 拋棄 — to leave and not come back」: the entry after its word, as DOCX and PDF print it. */
export function glossText(g: GlossLine): string {
  return [g.pos ? `(${g.pos})` : "", g.meaning_zh, g.definition_en ? `— ${g.definition_en}` : ""]
    .filter((p) => p !== "")
    .join(" ");
}

/**
 * A block's Glossary (詞彙級別檢查「提供字義」), printed at the end of the block. In the editor
 * an entry can be taken off the version shown.
 */
export function GlossaryBox({
  glosses,
  name,
  disabled = false,
  onRemove,
}: {
  glosses: GlossLine[];
  name: string;
  disabled?: boolean;
  onRemove?: (word: string) => void;
}) {
  if (glosses.length === 0) return null;
  return (
    <div className="ws-support ws-glossary" aria-label={`Glossary ${name}`}>
      <p className="ws-support-head">Glossary</p>
      <ul>
        {glosses.map((g) => (
          <li key={g.word}>
            <strong lang="en">{g.word}</strong> <span>{glossText(g)}</span>
            {onRemove && (
              <>
                {" "}
                <button
                  type="button"
                  className="text-link is-plain"
                  lang="zh-Hant"
                  disabled={disabled}
                  aria-label={`移除字義 ${g.word}`}
                  onClick={() => onRemove(g.word)}
                >
                  移除
                </button>
              </>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
