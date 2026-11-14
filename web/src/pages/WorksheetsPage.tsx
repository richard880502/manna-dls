import { Link } from "react-router";

/**
 * 學習單: its own destination (owner decision, 2026-11-14), not built yet. An honest placeholder
 * that says what is coming and offers nothing that does not work.
 */
export function WorksheetsPage() {
  return (
    <section aria-labelledby="worksheets-heading">
      <div className="page-head">
        <h1 id="worksheets-heading">學習單產生器開發中</h1>
        <p className="lead">
          之後可以在這裡貼上或選一篇教材，勾選想要的區塊（閱讀前、閱讀中、閱讀後與語言練習），由 Manna
          產生一份可以修改、可以列印的學習單；也可以從課例直接帶入這節課的目標與任務。這個功能還在開發，現在這裡沒有任何可以操作的東西。
        </p>
      </div>
      <p className="after-rows">
        現在可以先到<Link to="/library">備課庫</Link>備課，或回到<Link to="/">班級</Link>開始上課。
      </p>
    </section>
  );
}
