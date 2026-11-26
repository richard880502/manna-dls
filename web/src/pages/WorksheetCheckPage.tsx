import { Link, useParams } from "react-router";
import { ErrorText, Loading } from "../components/common";
import { VocabCheckPanel, useVocabCheck } from "../components/worksheet/VocabCheckPanel";
import { useDocumentTitle } from "../lib/useDocumentTitle";

/**
 * 詞彙級別檢查 as a page (`/worksheets/:id/check`): the same panel the workspace opens in a dialog.
 */
export function WorksheetCheckPage() {
  useDocumentTitle("詞彙級別檢查");
  const { worksheetId } = useParams();
  const id = Number(worksheetId);
  const ctl = useVocabCheck(id);
  const { state, loadError } = ctl;

  if (loadError) {
    return (
      <>
        <ErrorText message={loadError} />
        <p className="after-rows">
          <Link to="/prep">回我的作品</Link>
        </p>
      </>
    );
  }
  if (!state) return <Loading />;

  return (
    <>
      <div className="head-row page-head">
        <div>
          <p className="crumbs">
            <Link to="/prep">我的作品</Link> / <Link to={`/worksheets/${id}`}>{state.title || "未命名的學習單"}</Link>
          </p>
          <h1>詞彙級別檢查</h1>
          <p className="lead">對照大考中心高中英文參考詞彙表，看這份學習單的學生版用了哪些級別的字。</p>
        </div>
        <Link className="text-link" to={`/worksheets/${id}`}>
          回學習單修改
        </Link>
      </div>
      <VocabCheckPanel id={id} ctl={ctl} />
    </>
  );
}
