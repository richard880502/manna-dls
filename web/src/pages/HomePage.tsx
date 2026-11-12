import { UnitsPanel } from "../components/UnitsPanel";
import { useApp } from "../context/AppContext";

export function HomePage() {
  const { teacher } = useApp();
  return (
    <section aria-labelledby="home-heading">
      <div className="welcome-banner">
        <div>
          <p className="eyebrow warm">歡迎回來{teacher ? `，${teacher.research_id}` : ""}</p>
          <h2 id="home-heading">A journey of a thousand miles begins with a single step.</h2>
          <p>
            從一個單元開始：先寫單元前 CoRe，整理你最希望學生真正學會的重點；再逐課備課，Manna
            會提供附引用的建議，由你決定採用、修訂或拒絕。
          </p>
        </div>
        <div className="cycle-ring" aria-hidden="true">
          <strong>★</strong>
          <span>
            Study
            <br />& Plan
          </span>
        </div>
      </div>

      <div className="workflow-cards home-steps">
        <div className="workflow-card current">
          <span className="step-number">01</span>
          <div>
            <strong>單元前 CoRe</strong>
            <p>2–3 個學習重點 × 八題；可留白、可選「目前沒有想法」。</p>
          </div>
        </div>
        <div className="workflow-card current">
          <span className="step-number">02</span>
          <div>
            <strong>課例備課</strong>
            <p>連結 CoRe 重點、寫教案步驟，取得附引用的 RAG 建議。</p>
          </div>
        </div>
        <div className="workflow-card optional">
          <span className="step-number">03</span>
          <div>
            <strong>Teach &amp; Record</strong>
            <p>課中書籤（Phase 2 規劃中）</p>
          </div>
        </div>
        <div className="workflow-card optional">
          <span className="step-number">04</span>
          <div>
            <strong>Reflect &amp; Progress</strong>
            <p>課後反思與「下次要改」（Phase 2 規劃中）</p>
          </div>
        </div>
      </div>

      <UnitsPanel />
    </section>
  );
}

export function PlanIndexPage() {
  return (
    <section aria-labelledby="plan-heading">
      <div className="page-intro">
        <p className="eyebrow warm">01 · Study &amp; Plan</p>
        <h2 id="plan-heading">把初始的想法整理成可修改的教學規劃</h2>
        <p>
          選一個單元：先完成（或留白也可以）單元前 CoRe，再進入各課例備課。所有步驟都不會因為留白或略過 AI
          而被擋下。
        </p>
      </div>
      <UnitsPanel />
    </section>
  );
}
