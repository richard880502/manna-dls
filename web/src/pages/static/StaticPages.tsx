// Read-only ports of the handover prototype (Manna_DLS_工程交接_網站程式碼_20261109).
// These modules are Phase 2; controls are disabled and nothing is saved.
import type { ReactNode } from "react";
import { Phase2Tag } from "../../components/common";

function StaticIntro({ eyebrow, title, children }: { eyebrow: string; title: string; children: ReactNode }) {
  return (
    <div className="page-intro">
      <p className="eyebrow warm">{eyebrow}</p>
      <h2>{title}</h2>
      <p>{children}</p>
      <Phase2Tag />
    </div>
  );
}

/** Wrap prototype content so every control is inert. */
function Inert({ children }: { children: ReactNode }) {
  return (
    <fieldset className="static-fieldset" disabled aria-label="靜態展示（無法操作）">
      {children}
    </fieldset>
  );
}

const REFLECTION_LEVELS = [
  ["1 描述", "發生了什麼事？"],
  ["2 回應", "你當下怎麼想？"],
  ["3 連結", "這和原本預期一樣嗎？"],
  ["4 推理", "你怎麼知道？"],
  ["5 重構", "下一次想試什麼？"],
] as const;

export function ReflectPage() {
  return (
    <section>
      <StaticIntro eyebrow="03 · Reflect & Progress" title="從教學事件走向行動">
        回顧教學事件，接受 Manna 靈魂拷問。
      </StaticIntro>
      <Inert>
        <div className="reflect-grid">
          <section className="panel-card evidence-timeline">
            <div className="card-heading">
              <h3>Manna 建議：先確認「會用」，不只「懂意思」</h3>
              <span className="soft-tag">依據 3 筆 Teach &amp; Record</span>
            </div>
            <p className="field-hint">
              根據你的課堂紀錄，Manna 以「理解 → 使用」的 SIOP 教學邏輯，提出下一個可立即嘗試的小步驟。
            </p>
            <div className="timeline-item">
              <span>01</span>
              <div>
                <strong>你做得很好</strong>
                <p>你看見學生對 embarrassed 的意思有反應，並把這個片刻記下來。</p>
              </div>
            </div>
            <div className="timeline-item">
              <span>02</span>
              <div>
                <strong>你還不確定</strong>
                <p>你想到需要更多例句；但目前還不確定學生是只會對照詞義，還是能說出詞與情境的關係。</p>
              </div>
            </div>
            <div className="timeline-item active">
              <span>03</span>
              <div>
                <strong>你可能可以試試</strong>
                <p>保留改用生活情境說明的做法，並記下學生如何用自己的話連結情境與 embarrassed。</p>
              </div>
            </div>
          </section>
          <section className="panel-card manna-reflect">
            <div className="avatar-row">
              <div className="manna-avatar star-mascot">★</div>
              <div>
                <p className="eyebrow warm">Manna Asks</p>
                <h3>不是評分，是協助你看見判斷</h3>
              </div>
            </div>
            <div className="reflection-levels">
              <div className="card-heading">
                <div>
                  <p className="eyebrow warm">BAIN 5R</p>
                  <h3>選一個反思深度</h3>
                </div>
                <span className="soft-tag">可依需要挑選</span>
              </div>
              <div className="reflection-level-list">
                {REFLECTION_LEVELS.map(([label, title], i) => (
                  <button key={label} type="button" className={`reflection-level-button${i === 0 ? " is-selected" : ""}`}>
                    <strong>{label}</strong>
                    <span>{title}</span>
                  </button>
                ))}
              </div>
            </div>
            <div className="prompt-card">
              <strong>回應一個問題</strong>
              <p>今天教 embarrassed 時，學生對詞義和例句有什麼反應？</p>
              <textarea
                rows={4}
                readOnly
                aria-label="反思內容（示範）"
                defaultValue="我先請學生根據例句判斷 embarrassed 的意思。多數學生可以直接說出中文意思；我追問例句中的哪些線索讓他們這樣判斷時，他們能指出人物做錯事後感到尷尬的情境。"
              />
            </div>
            <label className="field-label" htmlFor="static-next-action">
              下次要改
            </label>
            <input
              id="static-next-action"
              readOnly
              defaultValue="設計一個看圖說話活動，確認學生能否在新情境自然用出 embarrassed。"
            />
            <div className="ai-decision">
              <span>AI 建議狀態：尚未決定</span>
              <button className="decision-button" type="button">
                接受
              </button>
              <button className="decision-button" type="button">
                修改
              </button>
              <button className="decision-button" type="button">
                拒絕
              </button>
            </div>
          </section>
        </div>
      </Inert>
    </section>
  );
}

export function SharePage() {
  return (
    <section>
      <StaticIntro eyebrow="04 · Share & Connect" title="把反思帶入真人共同探究">
        一個人走得快，一群人走得遠。
      </StaticIntro>
      <Inert>
        <div className="share-grid">
          <section className="panel-card share-preview">
            <div className="card-heading">
              <h3>我可以選擇分享</h3>
              <span className="optional-label">開啟功能</span>
            </div>
            {[
              ["教案", "分享備課版本、教學事件與下一步行動。"],
              ["學習單", "分享這次完成或修訂的課堂學習單。"],
            ].map(([title, desc]) => (
              <div key={title} className="share-item">
                <span className="share-icon">↗</span>
                <div>
                  <strong>{title}</strong>
                  <p>{desc}</p>
                </div>
                <label className="switch">
                  <input type="checkbox" aria-label={`分享${title}`} />
                  <span />
                </label>
              </div>
            ))}
          </section>
          <section className="panel-card peer-card">
            <div className="card-heading">
              <h3>Manna 推薦可能有興趣的夥伴</h3>
            </div>
            {[
              ["Julie 老師", "高職英文 · 句型與閱讀教學", "共同關注：單字教學", "/assets/avatars/julie-ai-thumb.jpg"],
              ["Chris 老師", "高中英文 · 課堂提問設計", "共同關注：英文口說", "/assets/avatars/chris-ai-thumb.jpg"],
            ].map(([name, desc, note, img]) => (
              <div key={name} className="peer">
                <div className="peer-avatar">
                  <img src={img} alt={`${name}的 AI 生成肖像`} />
                </div>
                <div>
                  <strong>{name}</strong>
                  <p>{desc}</p>
                  <small>{note}</small>
                </div>
                <button className="ghost-button peer-action" type="button">
                  邀請交流
                </button>
              </div>
            ))}
          </section>
        </div>
      </Inert>
    </section>
  );
}

export function ProfilePage() {
  return (
    <section>
      <StaticIntro eyebrow="Professional Profile" title="讓你看見我的努力">
        這是教師用來介紹自己、回顧專業成長與尋找合作夥伴的頁面（示範資料）。
      </StaticIntro>
      <Inert>
        <section className="profile-resume-header panel-card" aria-label="履歷摘要">
          <div className="profile-resume-head">
            <div>
              <p className="eyebrow warm">Professional summary</p>
              <h3>Mandy 老師</h3>
              <p className="profile-resume-role">高中英文教師 · 10 年教學經驗</p>
            </div>
            <span className="soft-tag">示範資料</span>
          </div>
          <p className="profile-resume-summary">
            我最近在嘗試把 SEL 結合英語教學，關注如何透過課堂提問與形成性評量，讓學生更願意參與學習。
          </p>
          <div className="profile-resume-meta">
            <div>
              <strong>核心專長</strong>
              <span>SEL 結合英語教學、課堂提問、形成性評量</span>
            </div>
            <div>
              <strong>合作意向</strong>
              <span>願意接受學校、教育計畫或教師社群邀請</span>
            </div>
          </div>
          <div className="profile-tags">
            {["SEL", "英語教學", "課堂提問", "形成性評量"].map((t) => (
              <span key={t}>{t}</span>
            ))}
          </div>
        </section>
        <div className="profile-grid">
          <section className="profile-section panel-card">
            <div className="profile-section-heading">
              <div>
                <p className="eyebrow warm">Keywords</p>
                <h3>個人關鍵字</h3>
              </div>
            </div>
            <div className="profile-tags">
              {["高中英文", "句型教學", "課堂提問", "共備"].map((t) => (
                <span key={t}>{t}</span>
              ))}
            </div>
          </section>
          <section className="profile-section panel-card">
            <div className="profile-section-heading">
              <div>
                <p className="eyebrow warm">Teaching expertise</p>
                <h3>教授課程</h3>
              </div>
            </div>
            <div className="profile-list">
              <div>
                <strong>高中英文</strong>
                <span>高一</span>
              </div>
              <div>
                <strong>選修課程</strong>
                <span>繪本演譯</span>
              </div>
            </div>
          </section>
          <section className="profile-section panel-card">
            <div className="profile-section-heading">
              <div>
                <p className="eyebrow warm">Recent learning</p>
                <h3>近期專業學習</h3>
              </div>
            </div>
            <div className="profile-list">
              <div>
                <strong>線上課程</strong>
                <span>形成性評量與課堂提問</span>
              </div>
              <div>
                <strong>一句話總結最近的學習</strong>
                <span>以學生需求為課程設計的出發點</span>
              </div>
            </div>
          </section>
          <section className="profile-section panel-card">
            <div className="profile-section-heading">
              <div>
                <p className="eyebrow warm">Community contribution</p>
                <h3>社群貢獻</h3>
              </div>
            </div>
            <div className="profile-list">
              <div>
                <strong>分享資源</strong>
                <span>2 份｜課堂提問與句型活動</span>
              </div>
              <div>
                <strong>參與共備／回應同儕</strong>
                <span>4 次共備、3 次回應</span>
              </div>
            </div>
          </section>
        </div>
      </Inert>
    </section>
  );
}
