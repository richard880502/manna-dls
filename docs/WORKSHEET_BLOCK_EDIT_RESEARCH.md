# 研究：學習單逐區修改、註記、仿作學習單的圖片，以及要不要 tool calling

日期：2026-11-17。只做研究，不改程式。程式位置以 `origin/main`（b2a90c8）為準。

主持人的問題：

1. 能不能像 Codex／ChatGPT canvas 那樣，直接在產生的學習單上註記，讓 AI 只改那一段？
2. 目前改學習單是整份改，需要逐塊改。
3. 照老師的學習單仿作時，應該要能產生圖片。
4. 這樣是不是要讓 LLM 有 tool calling 的能力？

另外兩項後續指示：(a) 「真實教師資料不能送國外」的限制暫時不考慮，把 Claude、OpenAI 都當成選項；(b) 圖片改用 OpenAI 的 image generation API（GPT Image 系列）。

> **實作狀態（2026-11-17）**：§4、§5（逐區修改與學習單註記）已實作，合約見 `docs/API.md`「學習單：逐區修改與註記」。與本文不同的地方：
> - 修改**不用** `worksheet_jobs`（§5.1 最後一段）：建議在採用前什麼都不寫，用 job 會鎖住編輯器、讓預覽變成進度條。執行狀態記在 `worksheet_block_proposals.status`（`running`），一份學習單同時只跑一個（partial unique index）。
> - `for_ai` 欄位命名為 `share_with_ai`，和教材註記的 API 一致；介面上一樣叫「提供給 AI」，預設勾選。
> - 採用時檢查的是「這一區和送出時完全相同」（存了 `base_block`），不是整份的 `base_version_no`：別的區塊之間改過不影響採用。
> - `PUT` 的版本檢查只在 `content` 和最新版本不同時才回 409。
> - §4.6「依註記修改（3 區）」依序跑多區沒有做：一次一區，另一區在跑時回 409，介面上其他區的按鈕會停用。
> - §6（圖片）由另一個 PR 處理。

## 0. 結論

| 問題 | 答案 |
|---|---|
| 1. 在學習單上註記 | **可以，而且大部分零件已經有了。** 區塊有穩定 id，也有逐區的歷史與「復原上一版」。教材頁的「選字 → 加註記 → 提供給 AI」可以照搬到學習單。要新增的是：註記的資料表、單一區塊的修改呼叫、預覽後再採用的流程。 |
| 2. 逐塊改 | 目前的 `prep_revise` 在**資料上已經是逐區**：輸出是「改了哪些 block」，每區單獨檢查、單獨進歷史。感覺像整份改，是因為入口和輸入都是整份：一個文字框對整份學習單，模型看到整份並自己挑要改哪幾區，改到的區塊要整區重寫，結果直接套用，沒有預覽。做法是讓老師先指定區塊（或選字），伺服器只送那一區給模型。 |
| 3. 仿作要有圖片 | 建議做「圖片位置」：文字模型寫出這裡需要什麼圖，伺服器呼叫 OpenAI GPT Image 產圖，存進我們的 MinIO，顯示在學習單並放進 DOCX／PDF；每張圖都可以單獨重畫或換成老師上傳的圖，失敗時退回空白框。預設模型建議 `gpt-image-2.5-flare`，品質 `medium`。一份學習單（3 個圖片位置、一半重畫一次）的圖片費用約 US$0.2。 |
| 4. 要 tool calling 嗎 | **不需要。** 改哪一區由老師點選決定，產圖由伺服器在檢查通過後呼叫。整個流程都是「結構化輸出（json_schema）＋伺服器編排」，gemma 和 Claude 都能跑，也沿用現有的檢查、重試和紀錄。要等到想做「一句話讓 AI 自己決定改哪裡、要不要配圖」的代理式對話，tool calling 才有價值，那時用 Claude 比用 gemma 穩。 |

改用 Claude 不會讓系統能產圖：Claude 只能看圖，不能產圖（見 §8）。圖片一定要另外接一個產圖服務，這次就是 OpenAI。

## 1. 現況：為什麼感覺是「整份改」

目前有三個修改入口：

| 入口 | 位置 | 範圍 | 老師能說要怎麼改嗎 |
|---|---|---|---|
| 備課對話底部「改學習單」 | `web/src/pages/PrepSessionPage.tsx:455-497`；後端 `api/src/prep/run.rs:1295`（`revise`）、prompt `prep_revise`（`api/migrations/0013_prep_sessions.sql:139-158`） | 整份學習單，由模型挑區塊（最多 6 區） | 可以，但要用文字描述位置（「B 區第 2 題」） |
| 編輯器每區的「重新產生」 | `web/src/components/worksheet/ResultView.tsx:213-221`；`POST /worksheets/:id/blocks/:block_id/regenerate`（`api/src/routes/worksheets.rs:748`、`api/src/worksheet/ai.rs:1787`） | 單一區塊 | **不行**，只是用原本的 prompt 重跑一次 |
| 編輯器每區的「復原上一版」 | `ResultView.tsx:222-231`；`undo_block`（`routes/worksheets.rs:759-830`） | 單一區塊 | 不適用 |

**已經是逐區的部分**

- 模型輸出是「區塊的修改清單」：`block_id` 是 schema 的 enum，只能選這份學習單裡的區塊（`api/src/prep/mod.rs:505-543`）。
- 每一區分別用 `evaluate_section` 檢查，沒通過的區塊維持原樣，其他區照常套用（`run.rs:1095-1162`）。
- 被改掉的區塊先放進 `worksheet_block_history`，所以編輯器的「復原上一版」可以逐區還原（`run.rs:1414`、`api/src/worksheet/store.rs:517-569`）。
- 回覆裡有 `changed_block_ids`，對話頁會讓改過的區塊閃一下（`PrepSessionPage.tsx:80-97`、`SheetPreview.tsx`）。

**讓老師覺得是整份改的原因**

1. **沒有指定區塊的方法。** 只有一個文字框，針對整份學習單。placeholder 寫的是「針對上面產出的內容，你有什麼想修改的嗎？」（`web/src/lib/prep.ts:61`）。學習單預覽的每一區只有「複製／有幫助／不合用」（`web/src/components/prep/SectionTools.tsx`），沒有「改這一區」。
2. **模型拿到的是整份，也由它決定改哪裡。** `blocks_for_model` 把所有啟用的區塊連同全部題目送出（`run.rs:1165-1186`），prompt 要模型自己判斷，並把「A 區」「B 區」對回 label（`0013_prep_sessions.sql:144`）。模型可能多改，也可能改錯區。
3. **改到的區塊要整區重寫。** prompt 第 3 條要求寫出改後完整的 title、instructions 和全部 items，「沒有改的題目也照原樣抄回來」（`0013_prep_sessions.sql:145`）。只想改一題，整區仍會重寫：
   - gemma 抄回來的題目可能有小改動；
   - 所有題目的 id 都換新（`{block}-v{turn}i{k}`，`run.rs:1189-1196`），`origin` 一律變成 `ai`（`run.rs:1258`），老師之前手改過的痕跡（`ai_edited`）也跟著消失。
4. **直接套用，沒有預覽。** 全部改動寫成一個新版本（`cause = "revise"`），整張預覽重新載入（`WorksheetCard` 的 `refreshKey`，`PrepSessionPage.tsx:231-241`）。在對話頁沒有逐區的「不要這個改法」，要到編輯器才有「復原上一版」。
5. **位置用字母描述不可靠。** A、B 是投影時依順序算出來的（`api/src/worksheet/assemble.rs:561`）。區塊被移動或關掉後，字母就變了。
6. **只能改最新的一份。** 底部文字框永遠改對話裡最新做出的那份學習單（`api/src/prep/store.rs:437-439`）。

## 2. 可以沿用的零件

- **區塊 id 穩定**：`warmup`、`exit`、任務區塊、版型區塊 `r{run}b{j}`（`api/src/worksheet/ai/template.rs:432`）。存在 `worksheet_versions.content` 的 JSONB 裡，移動、改字都不會變。題目 id 會在 revise 和重新產生時更換，所以註記要像教材註記那樣存「引文＋前後文」，讀取時重新定位。
- **給前端的學習單投影沒有題目 id**：`WsSheetBlock.items` 只有 `no`（`web/src/api/types.ts:1806-1818`），要補上 `id`（改 `assemble::project`，工作量小）。
- **教材註記可以照搬**：
  - 錨點 = 段落 id + UTF-16 位移 + `quote`／`prefix`／`suffix`，每次讀取時重新定位，找不到就請老師重新確認（`api/src/materials/annotations.rs:1-7`）。
  - `share_with_ai` 預設關閉，只有勾了的才會給模型（`annotations.rs:461`、`api/migrations/0009_materials.sql:55-77`）。
  - 給模型時有固定標題，標明這是教師筆記，不是教材原文（`annotations.rs:507-530`）。
  - 前端的選字工具：`selectionInParagraph` 依 `data-pid` 找段落並對齊整個英文字（`web/src/lib/materials.ts:195-230`），註記卡片與「提供給 AI」勾選在 `MaterialViewer.tsx:120-215`。
  - 用字：已在 PR #43 統一為「註記」。
- **區塊檢查**：`evaluate_section` 檢查固定標籤、選擇題格式、題幹不含答案、音節對字典、需要外部素材時不得編造（`api/src/worksheet/checks.rs:1395`）。
- **需要外部素材的區塊**：版型的 `needs_external_material` 會讓區塊顯示 `MSG_EXTERNAL`「Manna 不會自己編造」（`api/src/worksheet/template/mod.rs:49,123`），編輯器會統計「有 N 個區塊需要你提供素材（影片、錄音、圖片…）」（`ResultView.tsx:728-730`）。這就是「圖片位置」的前身。
- **範例學習單裡的圖**：chandra OCR 的 `Picture` 區塊連同 bbox 都存在 `materials.ocr_pages`，只是沒變成文字（`api/src/materials/ocr/html.rs:14,30-50`、`docs/API.md:2255`）。這只限掃描檔與圖片；DOCX 和有文字層的 PDF 不跑 OCR，裡面的圖目前完全沒讀。
- **檔案儲存與 `<img>` 簽名連結**：`Storage`（S3／MinIO 或本機）與 `LinkSigner`（`api/src/lib.rs:57-61`）。
- **繞過閘道快取**：重新產生時提高溫度，讓請求和上一次不同（`api/src/worksheet/ai.rs:442`、`api/src/ai/mod.rs:31-34`）。

## 3. 參考產品怎麼做

| 產品 | 互動 |
|---|---|
| ChatGPT canvas | 選取一段文字後出現「Ask ChatGPT」（較新版本寫「Ask for changes」），針對選取的部分要求修改或提問。也可以在選取處留言，再按 Apply 讓 AI 依留言修改。建議會標在原文旁邊，可以逐條採用或忽略。 |
| OpenAI Codex | 在 review pane 的 diff 上逐行留 inline comment，再送一句「處理這些註解，範圍保持小」，然後在 diff 裡檢查結果。註解綁在當下的 diff，內容改了位置可能跑掉。 |
| Google Docs（Gemini） | 選取文字 → Refine（Rephrase、Shorten、Custom…）→ 預覽 → 採用或取代。 |
| MagicSchool（Studio Mode） | 左邊是可以直接編輯的學習單，右邊是 AI 對話。可以只重新產生單一題，其他題不動。 |
| SchoolAI | 選取文件的一段來重新下指令。 |

這些產品的共同點：

1. 錨點就是老師的選取；
2. 指示附在錨點上；
3. AI 只改錨點的範圍；
4. 先預覽差異，再逐條採用或放棄；
5. 留言保留下來，標成已處理。

## 4. 建議的互動

**在備課對話的學習單預覽（主要路徑）**

1. 每一區的工具列（`SectionTools`）加「改這一區」。選取區塊裡的文字時，跳出小浮框「加註記」。浮框沿用 `Popover` 和選字邏輯；預覽要在標題、指示、每一題上加 `data-anchor` 屬性（例如 `title`、`instructions`、`item:<id>`）。
2. 註記卡片：
   - 註記內容；
   - 「請 Manna 照這則註記改」：對應教材的「提供給 AI」，這裡預設打勾；
   - 送出。
3. 只有那一區顯示「Manna 正在改這一區…」，其他區不動，也不重新載入整張學習單。
4. 結果直接顯示在原區塊：刪掉的題目加刪除線，新題目標記出來，並附模型的一句話說明。下面三個按鈕：「採用」「不採用」「再改一次」。採用後，那一區照常可以「復原上一版」。
5. 註記留在區塊下方（就像教材註記列在段落下方），狀態是「待處理／已依此修改／已略過」。註記不印在學生版，也不放進匯出。
6. 同一區有好幾則註記時，合成一次呼叫。不同區各有註記時，按「依註記修改（3 區）」依序一區一區跑，不要並行（見 §5.4）。
7. 底部文字框保留，用來下整份的指示（例如「全部改成中英並用」）。老師先點選一區時，文字框前面出現一個標籤「針對 B 區」，這時就走單區修改。

**在編輯器（`/worksheets/:id`）**：「重新產生」旁邊加「依註記修改」，用同一套 API。編輯器的題目本來就是可編輯的文字框，所以註記只做到「這一題」或「這一區」，不做選字。

## 5. API 與資料模型草案

### 5.1 資料表

```sql
-- 學習單上的註記：不改學習單本身，和教材註記同一套錨點
CREATE TABLE worksheet_comments (
    id            BIGSERIAL PRIMARY KEY,
    worksheet_id  BIGINT NOT NULL REFERENCES worksheets (id),
    teacher_id    BIGINT NOT NULL REFERENCES teachers (id),
    block_id      TEXT NOT NULL,
    field         TEXT NOT NULL,          -- 'block' | 'title' | 'instructions' | 'item'
    item_id       TEXT,                   -- field = 'item'；題目換 id 後靠 quote 重新定位
    start_offset  INT, end_offset INT,    -- UTF-16，選字時才有
    quote         TEXT NOT NULL DEFAULT '', prefix TEXT NOT NULL DEFAULT '', suffix TEXT NOT NULL DEFAULT '',
    note          TEXT NOT NULL DEFAULT '',
    for_ai        BOOLEAN NOT NULL DEFAULT true,
    status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'applied', 'dismissed')),
    proposal_id   BIGINT,                 -- 處理它的那次修改
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 模型提出、老師還沒決定的單區修改
CREATE TABLE worksheet_block_proposals (
    id               BIGSERIAL PRIMARY KEY,
    worksheet_id     BIGINT NOT NULL REFERENCES worksheets (id),
    block_id         TEXT NOT NULL,
    base_version_no  INT NOT NULL,        -- 以哪一版為底；那一區之後被改過就變 stale
    proposed         JSONB NOT NULL,      -- 改好的 Block
    reply            TEXT NOT NULL,
    comment_ids      BIGINT[] NOT NULL DEFAULT '{}',
    interaction_id   BIGINT REFERENCES ai_interactions (id),
    status           TEXT NOT NULL CHECK (status IN ('proposed', 'accepted', 'rejected', 'stale')),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(), decided_at TIMESTAMPTZ
);
```

背景工作沿用 `worksheet_jobs`，`step` 多一個 `block_revise`，這樣「正在產生」的鎖和輪詢都照舊。`step` 的 CHECK 已經擴充過兩次（`0012`、`0014`），再加一個值即可。

### 5.2 端點

| 方法 | 路徑 | 作用 |
|---|---|---|
| GET／POST | `/api/worksheets/:id/comments` | 列出註記（讀取時重新定位）；新增 `{block_id, field, item_id?, start?, end?, note, for_ai}`，伺服器從目前內容取 `quote`／`prefix`／`suffix`，和教材註記一樣 |
| PATCH／DELETE | `/api/worksheets/:id/comments/:cid` | 改內容、改狀態、刪除 |
| POST | `/api/worksheets/:id/blocks/:block_id/revise` | `{comment_ids?: [], instruction?: string}` → 202，開一個 `block_revise` job；跑完產生一筆 proposal |
| POST | `/api/worksheets/:id/proposals/:pid/accept` | 確認那一區自 `base_version_no` 後沒變，然後 `push_history(舊區塊)` → `save_content(cause = 'revise')` → 註記標成 `applied` |
| POST | `/api/worksheets/:id/proposals/:pid/reject` | 記一筆 `teacher_decisions`（`reject`），註記回到 `open` |
| POST | `/api/worksheets/:id/blocks/:block_id/undo` | 現有，不用改 |

採用、放棄、復原都寫進 `teacher_decisions`。老師的註記和決定，本身就是研究要的「教師如何修正 AI 產出」資料。

### 5.3 送給模型的內容（新 prompt key `worksheet_block_revise`）

**輸入**

- 目標區塊的完整內容，題目帶 id；
- 這一區的註記：引文、位置、說明；
- 其他區塊的標題與題幹（唯讀，避免重複，也讓「不要和 C 區重複」這類指示能做到）；
- 教材，帶段落標記；
- 若是照版型做的：該版型區塊的 `instruction_style`、`item_patterns` 與整份的 `style_notes`，讓改完仍維持老師的風格；
- 區塊的 Bloom 層次、班級特性、老師勾了「提供給 AI」的教材註記。

不送學生資料。

**輸出 schema（屬性依寫出順序排列）**

```text
{ title ≤, instructions ≤,
  items ≤12: [{ source: enum(<這一區的題目 id>…, "new"), keep: boolean,
               answer_format: enum, prompt ≤, options ≤, answer ≤, bloom: enum }],
  image_slots ≤2: [...],            // §6，第二期
  reply ≤200 }                      // 放最後：先改，再說明改了什麼
```

`keep = true` 時，伺服器直接沿用原題（id、`origin`、`levels`、`hint` 都不變），不採用模型抄回的文字。這解決了現在「整區重抄」造成的小改動與 id 更換。

**範圍由程式管，不靠 prompt**：註記錨在某一題時，其他題一律強制 `keep`。只有老師勾「可以動到同一區其他題」才放寬。

### 5.4 檢查、重試與閘道的坑

- 合併後的區塊交給 `evaluate_section`。`SectionCtx.item_count` 取 `max(目前題數, 12)`，`needs_external` 取版型區塊的設定。沒通過就用改過的請求重試一次，做法同 `run_step`；仍失敗就不產生 proposal，回覆說明原因。
- **json_schema 的欄位依 schema 順序寫出**：`reply` 放最後，`keep` 放在題目內容前面。
- **每個字串和陣列都要設 `maxLength`／`maxItems`**，沿用 `s_max`、`strings_max`。
- **gemma 想換行時會直接結束字串**：多行內容（例如步驟、鷹架）用字串陣列，一行一項（同 `RawAnswer`，`api/src/prep/mod.rs:346`）。
- **相同請求會被閘道快取**：「再改一次」時溫度要調高，或把嘗試次數放進請求，否則會拿到同一個答案。
- **同一份東西的不同部分並行呼叫會互相重複**（`api/src/worksheet/ai/bloom.rs:745-746`）：多區修改要依序跑，後面的呼叫看得到前面已改好的內容。

### 5.5 和編輯器自動儲存的衝突

`PUT /api/worksheets/:id` 用整份 content 存檔，後存的蓋掉先存的，不檢查版本（`routes/worksheets.rs:554-636`）。編輯器開著、還有沒存的修改時，另一邊採用了單區修改，可能被編輯器下一次自動儲存蓋回去。

建議：

- `PUT` 加選填的 `base_version_no`，版本不符回 409；
- 前端在送出修改與採用之前先 `flush()`，做法同現有的重新產生（`web/src/components/worksheet/useWorksheet.ts:160-170`）。

## 6. 圖片

### 6.1 選項比較

| 選項 | 優點 | 缺點 | 工作量 |
|---|---|---|---|
| **A. 從老師的範例裁圖**（chandra 的 `Picture` bbox；DOCX 讀 `word/media`） | 真的是老師的圖；不必再付費產圖 | 範例的圖多半屬於舊課，套到新課常常不對，只適合裝飾性或版面用的圖示；bbox 的座標系要先實測（像素還是 0–1000）；DOCX 與文字層 PDF 要另外抽圖 | 中 |
| **B. 圖片位置，老師補圖**（AI 寫明這裡需要什麼圖） | 不會亂編；延續現有「需要你提供素材」；任何模型都能做 | 老師還是要自己找圖 | 小到中 |
| **C. Openverse／Wikimedia Commons 搜圖** | 真實照片，適合地圖、地標、動物；免費 | 相關性不穩；CC BY／BY-SA 要標示出處，ND 不能裁切，NC 要確認用途 | 中 |
| **D. 圖示與程式畫的簡單圖（SVG）** | 穩定、離線、印刷清楚；模型從固定清單（enum）挑圖示即可 | 不是內容插圖 | 小 |
| **E. OpenAI GPT Image 產圖**（主持人指定） | 依需要產出合用的插圖，例如情境圖、字卡圖、看圖說話 | 每張要付費；最多可能等 2 分鐘；圖中文字常錯；不適合需要真實準確的圖（歷史照片、地圖、國旗） | 中 |
| F. 國網中心的產圖模型 | 資料留在國內 | 目前沒有已知可用的模型，要問國網中心 | — |

**建議：做 B 的資料模型，用 E 填圖。** 「圖片位置」是核心：每個位置可以由 GPT Image 產圖、老師上傳、從範例裁圖（A，第三期）或搜圖（C，選做）來填。產圖失敗或被擋時，就是一個有說明的空框，等老師補圖。

### 6.2 OpenAI GPT Image：查到的事實

- **主持人給的連結**（`openai.com/zh-Hant/index/image-generation-api/`）用工具抓取時回 403，沒能逐字核對。依網址判斷，應是 2025 年 `gpt-image-1` 上架 API 的公告。**`gpt-image-1` 已於 2026-10-23 停用**，`gpt-image-2` 與 `gpt-image-1-mini` 也已標為 deprecated。同一支 API 現在用的是 2.5／2.6 系列。
- **目前的模型**：
  - `gpt-image-2.5-flare`：快速的日常產圖；
  - `gpt-image-2.5-sunburst`：能力最強；
  - `gpt-image-2.6-flare`：快速迭代；
  - `gpt-image-2.6-sunburst`：細節工作。
  - 模型汰換很快，**模型 id 要做成環境變數**。
- **端點**：`POST /v1/images/generations`，回應是 `data[0].b64_json`。
- **參數**：
  - `size`：`1024x1024`、`1536x1024`、`1024x1536`（2.x 也接受自訂尺寸，邊長是 16 的倍數，最大 3840x2160）；
  - `quality`：`low`／`medium`／`high`；
  - `output_format`：`png`／`webp`／`jpeg`；
  - `background`：可設 `transparent`（需 png 或 webp）；
  - `moderation`：`auto`（預設，會限制不適合未成年的內容）或 `low`；
  - `n`：1–10。
- **價格**：2.5／2.6 系列與 `gpt-image-2` 同價：文字輸入 $5、圖片輸出 $30（每百萬 tokens）。2.5／2.6 沒有公布每張圖的 token 數；`gpt-image-2` 公布的每張價格可以當估計：

  | 品質 | 1024x1024 | 1536x1024 或 1024x1536 |
  |---|---|---|
  | low | $0.006 | $0.005 |
  | medium | $0.053 | $0.041 |
  | high | $0.211 | $0.165 |

  上線後以回應裡的 `usage` 核實。
- **速率限制**：`gpt-image-2` 的模型頁列出 Build 級每分鐘 20 張、Launch 150 張、Grow 250 張；2.6 的模型頁沒列，以帳號的 limits 頁為準。
- **組織驗證**：官方指南寫使用 GPT Image 模型「may need to complete the API Organization Verification」。社群回報沒驗證會收到 403。驗證要由帳號負責人透過 Persona 做證件驗證，**建議在 12/15 前先完成**。
- **資料**：API 資料預設不拿去訓練。`/v1/images` 的濫用監控紀錄保留 30 天。2.5 系列、`gpt-image-2`、`gpt-image-1.5` 可用 Zero Data Retention；2.6 不在官方文件的清單上。這是選 `gpt-image-2.5-flare` 當預設的理由之一。
- **官方列出的限制**：
  - 圖中文字的位置和清晰度仍不穩；
  - 同一角色在多張圖之間不容易保持一致；
  - 複雜的提示可能要等到 2 分鐘。

### 6.3 整合草案

**設定**（仿 `api/src/materials/ocr/client.rs`）

- `IMAGE_PROVIDER=openai|fake|none`（預設 `none`），`IMAGE_MODEL`（預設 `gpt-image-2.5-flare`），`IMAGE_QUALITY`（預設 `medium`），`IMAGE_SIZE`（預設 `1536x1024`），`IMAGE_TIMEOUT_SECS`（120），`IMAGE_CONCURRENCY`（2）。
- 金鑰 `OPENAI_API_KEY` 由主持人在 Coolify 設定，不進版本庫。
- 和 OCR 一樣是選用功能：設定錯誤只會關掉產圖並發出警告，不會讓伺服器起不來。
- 設定的 `Debug` 不印出金鑰；錯誤訊息只帶 HTTP 狀態，不帶回應內容和網址（同 `OcrConfig`）。

**誰寫圖片描述**：寫區塊的那個文字模型（gemma 或 Claude）在同一次結構化輸出裡多寫 `image_slots`，每區最多 2 個：

```text
image_slots: [{ kind: enum("scene","object","icon","photo_needed"),
                purpose_zh ≤80,     // 給老師看：這張圖在這裡做什麼
                prompt_en ≤300,     // 給產圖模型：只描述畫面
                alt_en ≤120,        // 替代文字
                position: enum("after_instructions", "before_items", "item:<id>") }]
```

`photo_needed` 代表需要真實準確的照片（地圖、國旗、真實地標、歷史事件）。這種位置**不產圖**，只留空框請老師補，或之後接搜圖。

**流程**

1. 區塊通過 `evaluate_section` 後，伺服器檢查 slot：長度、enum，以及不得出現人名、品牌、網址（沿用 `names_a_resource` 這類檢查）。
2. 每個 slot 開一個 `image` job，依序跑，最多 `IMAGE_CONCURRENCY` 個。
3. 呼叫 OpenAI，`prompt` = 固定風格前綴（§6.4）+ `prompt_en`。
4. 解碼後轉成 webp，存到 MinIO：`worksheet-images/{worksheet_id}/{image_id}.webp`。
5. 前端用 `LinkSigner` 的短期連結顯示。

**資料**

- 新表 `worksheet_images`，記錄：`worksheet_id`、`block_id`、`slot_id`、`source`（`generated`／`upload`／`sample_crop`）、`storage_key`、尺寸、送出的 prompt、模型、`interaction_id`、`status`（`pending`／`ready`／`failed`／`blocked`）。
- 區塊裡記 `image_slots` 和目前採用的 `image_id`。
- `ai_interactions` 記一筆 `module = worksheet_image`：模型、尺寸、品質、耗時、`usage`、結果（是否被 `moderation_blocked`）。不存金鑰；圖片本身只記大小和 SHA-256，做法同 OCR。

**單張操作**

- 「重畫這張」：帶 `attempt`，提示可附老師的一句話；
- 「換成我的圖」：上傳，沿用現有的檔頭檢查與大小上限；
- 「拿掉」：刪除這張圖。

這三個操作都只動那一張圖，舊圖保留在 `worksheet_images`，可以換回來。

**失敗與退回**：逾時、5xx、被安全機制擋下或沒設定金鑰時，slot 顯示虛線框和 `purpose_zh`（「請放一張：…」），附「重畫」「上傳」兩個按鈕。匯出時印成同樣的空框：教師版附說明，學生版只有框。

**匯出**

- DOCX：在 zip 裡加 `word/media/*`、relationship 和 inline drawing（`api/src/worksheet/export.rs` 目前沒有圖）。
- PDF：要開 `krilla` 的點陣圖功能（目前 `default-features = false, features = ["simple-text"]`，`api/Cargo.toml:28`）。
- 這是圖片這一期最大的單項工作。

### 6.4 內容安全與風格一致

- **固定風格前綴**（存在 `prompt_versions`，key 為 `image_style`，可以在管理頁改，每次產圖記下用的是第幾版）。例如：

  > Simple flat vector illustration for a Taiwanese senior-high English worksheet. Clean outlines, soft limited palette, plain white background, one clear subject, classroom-appropriate. No text, letters, numbers, labels, logos, watermarks or brand names. No real or famous people.

- **圖裡不放字**：官方也承認圖中文字不可靠。需要的英文單字、標籤、編號，由我們的版面印成圖說，不畫在圖裡。
- **`moderation: auto`**（預設），不要用 `low`。被擋下時給老師一句一般性的說明，`moderation_details` 只進紀錄。
- `safety_identifier` 用研究編號的雜湊，不放姓名。
- 文字模型產生的 `prompt_en` 也要過伺服器檢查（人名、品牌、網址、長度）；`kind = photo_needed` 不產圖（見 §6.3）。
- **一份學習單用同一個風格前綴、同一個尺寸**。要更一致時，第二期之後可以把第一張圖當參考，改用 `/v1/images/edits`。
- **標示 AI 生成**：教師版每張圖下面標「AI 生成」。學生版要不要標，請主持人決定。
- **老師把關**：產出的圖在老師看過之前不放進匯出？還是直接放？建議直接放，但匯出前提示「有 N 張 AI 產生的圖還沒看過」。

### 6.5 費用估計

假設：

- 一份照版型做的學習單約 8 區，每次產生呼叫輸入約 4,000 tokens、輸出約 1,500 tokens；Claude 另有思考 tokens，以 low／medium effort 估每次多 1,000 tokens。
- 老師用註記改 3 次，每次輸入約 5,000、輸出約 2,500 tokens。
- 抽版型 2 次呼叫，每份範例只做一次。
- 3 個圖片位置，`medium` 1536x1024，其中一半重畫一次，約 4.5 張。

價格來源：Anthropic pricing 頁、OpenAI pricing 頁與 image generation 指南。

| 項目 | 估計（US$） |
|---|---|
| 文字：Claude Sonnet 5.5（$2／$10 每百萬 tokens） | 產生 ≈ 0.26 ＋ 註記修改 ≈ 0.11 ＝ **≈ 0.37**；抽版型首次另加 ≈ 0.10 |
| 文字：Claude Opus 5.5（$4／$20） | ≈ 0.74；抽版型首次另加 ≈ 0.20 |
| 文字：Claude Haiku 5.5（$0.10／$0.50） | ≈ 0.02 |
| 文字：gemma（國網中心） | 依國網中心計費與配額（未知） |
| 看範例學習單的圖（Claude 視覺，4 頁 × 約 1,600 tokens） | ≈ 0.013，每份範例一次 |
| 圖片：GPT Image `medium`，4.5 張 | **≈ 0.18–0.24** |
| 圖片：`low`，4.5 張 | ≈ 0.03 |
| 圖片：`high`，4.5 張 | ≈ 0.75–0.95 |

合計：

| 組合 | 每份 | 每月 200 份（50 位老師 × 4 份） |
|---|---|---|
| Sonnet 5.5 ＋ medium 圖 | ≈ US$0.55–0.6（約 NT$18–20，以 1 USD ≈ 32 TWD 粗估） | ≈ US$120 |
| gemma ＋ medium 圖 | 圖片 ≈ US$0.2，另加國網中心費用 | 圖片 ≈ US$40–50 |

產圖的提示文字（每張約 200 tokens × $5／百萬）可以忽略。

## 7. tool calling

**對問題 1–3 都不需要。**

- 逐區修改：要改哪一區由老師點選決定，不需要模型「選工具」。
- 產圖：伺服器在檢查通過後依 `image_slots` 呼叫 OpenAI，不需要模型「選工具」。
- 現有的 json_schema 結構化輸出已經能限制輸出、套 enum、做伺服器檢查和重試，而且每一步都能記進 `ai_interactions`。

**tool calling 能多做什麼**

- 代理式的對話：一句「把 B 區改簡單一點，C 區前面加一張圖」，模型自己決定呼叫 `revise_block`、`add_image_slot`，看結果再決定下一步。
- 搜圖後讓模型看候選圖再挑一張。

前者其實也可以不用工具：先用一次結構化呼叫產出「操作清單」，再由伺服器逐項執行。這等於沒有迴圈的 tool calling，gemma 也能跑。

**如果要在國網中心的 gemma 上用 tools**（要實測，閘道設定我們不知道）：

- vLLM 的 Gemma 4 食譜要求 `--enable-auto-tool-choice --tool-call-parser gemma4 --chat-template examples/tool_chat_template_gemma4.jinja`（另有 `--reasoning-parser gemma4`），vLLM ≥ 0.19.1。另有一個「帶 tools 的請求回 400」的 parser 錯誤，在 PR #38847 修掉。
- `tool_choice: "auto"` 時，參數只有在工具設了 `strict: true` 而且伺服器開了 `VLLM_ENFORCE_STRICT_TOOL_CALLING` 時才受 schema 限制，否則參數可能是壞的 JSON。
- 指定工具或 `required` 走 structured outputs，可能不需要那些旗標就能用，但效果和我們現在的 json_schema 一樣，沒有多得到什麼。
- 有回報指出，在長 system prompt、多輪、多工具的情況下，Gemma 4 有時會把工具呼叫寫成一般文字。

**用 Claude 時**

- 工具呼叫穩定很多，也支援 `strict: true`。
- 但 Opus 5.5 與 Sonnet 5.5 不接受強制的 `tool_choice`（`any`／`tool` 會回 400），只能用 `auto` 加提示，或直接用結構化輸出。
- 現在的 `LlmRequest` 沒有 `tools` 欄位（`api/src/ai/mod.rs:20-35`），要加工具迴圈，就要改 `openai.rs`、`anthropic.rs` 和 pipeline 的逐步紀錄。

## 8. 改用 Claude 可以解決什麼

- **Claude 不能產圖。** 官方 Vision 文件的說法是「Claude is an image understanding model only」。它能做的有：
  - 用程式碼寫 SVG 或簡單圖表；
  - **看圖**：讀懂範例學習單每一頁和每張圖，判斷那是什麼圖、放在哪裡、做什麼用，用來寫圖片位置的描述；
  - 呼叫我們定義的工具（搜圖、產圖）。
- **所以只換成 Claude，不會多出產圖能力**，圖片還是要接 OpenAI（§6）。
- **Claude 真正有幫助的地方**：
  - 複雜指示和 tool calling 比 gemma-4-31B 可靠；
  - 能看範例的圖；
  - gemma 的幾個 json 怪癖不會發生（字串遇換行就結束、照 schema 順序寫等）。
  - 要比較就用同一組 10 份範例（`docs/WORKSHEET_TEMPLATE_PROBE.md`、`docs/llm-eval/worksheet-real/`）實測，不要用猜的。
- **gemma 也可能可以看圖**：vLLM 食譜說 `gemma-4-31B-it` 原生支援圖片輸入，但國網中心的閘道有沒有開放，要問。

**切換要改的程式**（不在這次範圍，只列出來）：

1. **`AI_PROVIDER=anthropic` 必須同時設 `DEMO_MODE=true`**：`api/src/config.rs:52-57`（錯誤訊息）、`212-219`（檢查）。
   - 示範站現在設 `DEMO_MODE=true` 就能用 Claude，不用改程式。
   - 正式站要用 Claude，得改掉這個開關，也要拿掉每頁的「示範環境：請勿輸入真實教師或學生資料」（`web/src/components/common.tsx:12`）。
2. **文件與同意書**：`docs/BLUEPRINT.md:61-66`（「正式平台只能用國網中心核准的模型」）、`README.md:43`、`docs/API.md`；教師看到的隱私與同意說明（`teachers.consent_scope`）要寫明會送到哪些境外服務（Anthropic、OpenAI）。
3. **schema 相容性**（實際上最可能出錯的地方）：
   - 依 Anthropic 文件，structured outputs 不支援 `minLength`／`maxLength` 和複雜的陣列限制。
   - 學習單、版型、備課對話的 schema 都大量用了 `maxLength`／`maxItems`／`minItems`（`s_max`、`strings_max`，`api/src/worksheet/template/mod.rs:368-374`）。
   - `anthropic.rs` 目前把 schema 原樣送出（`api/src/ai/anthropic.rs:20-31`）。舊的 `ai/schemas.rs` 有測試擋掉這些鍵（`schemas.rs:162`），新的 schema 沒有。
   - 所以要在 Anthropic 這條路上，送出前拿掉不支援的鍵。伺服器本來就會截斷和檢查長度，影響不大。要實測。
4. **只有一個全域的 AI provider**（`AiClient::from_config`，`api/src/lib.rs:72`）。要「某些步驟用 Claude、其他用 gemma」，就要依 prompt key 選 provider。

**合規**：暫時不考慮；正式上線前需主持人確認規格與同意書。

## 9. 建議架構

| 部分 | 建議 |
|---|---|
| 逐區修改＋註記 | 註記存在 `worksheet_comments`；單區呼叫 `worksheet_block_revise`，用結構化輸出，題目標 `keep`／`source`；`evaluate_section` 檢查；產生 proposal，老師採用後才寫入，採用後可「復原上一版」。文字模型可抽換，gemma 和 Claude 都能跑。 |
| 圖片位置 | 寫區塊的文字模型順便寫 `image_slots`；看範例圖的描述，由視覺模型（Claude；國網中心若開放圖片輸入，也可用 gemma）在抽版型時寫；產圖用 OpenAI `gpt-image-2.5-flare`、medium，伺服器呼叫；存 MinIO；逐張重畫或替換；失敗退回空框。 |
| tool calling | 不用。等要做代理式對話時再評估，到時候用 Claude。 |
| 12/15 示範 | 在示範站（不是正式站 `manna`）設 `AI_PROVIDER=anthropic`、`DEMO_MODE=true`、`IMAGE_PROVIDER=openai`。先處理 §8 第 3 點的 schema 相容性，並用 10 份範例跑一輪。如果 Claude 沒有明顯比較好，示範就用 gemma 加 OpenAI 產圖。 |
| 正式上線 | 架構不變，只換設定。文字模型依實測結果和主持人的合規決定選擇；產圖預設開，每位老師設每月張數上限。 |

## 10. 分期與工時（一位工程師加 Claude Code 的粗估）

| 期 | 內容 | 工時 |
|---|---|---|
| 0 | 主持人做決定（§11）；完成 OpenAI 組織驗證；在 Coolify 設金鑰；問國網中心的問題 | 0.5 天，加上等待 |
| 1 | 逐區修改＋註記：兩張表、端點、prompt v1、`keep`／`source` schema、proposal 採用／放棄、對話預覽的選字和「改這一區」、編輯器按鈕、學習單投影補題目 id、`PUT` 版本檢查、假模型測試 | 5–7 天 |
| 2 | 圖片位置＋OpenAI：`IMAGE_PROVIDER` client、`worksheet_images`、存檔與簽名連結、空框／重畫／上傳 UI、DOCX 與 PDF 放圖、紀錄與費用統計 | 5–7 天（匯出約佔一半） |
| 3 | 仿作的圖：抽版型時找出範例裡的圖（OCR 的 `Picture` bbox、DOCX 的 `word/media`），用視覺模型描述，對應到版型區塊的 slot；裝飾性的圖可以直接裁用 | 3–5 天 |
| 4（選做） | Claude 的 schema 相容性與依 prompt key 選 provider；代理式對話（tool calling） | 2–3 天＋視需求 |

12/15 前做完第 1、2 期是可行的，第 3 期可以先做「看範例的圖並寫成 slot 描述」。

## 11. 待決定與要問的事

**請主持人決定**

1. 註記修改要先預覽再採用（建議），還是像現在一樣直接套用、再提供復原？
2. 註記能不能選字，還是只到「這一題／這一區」？建議預覽頁可以選字，編輯器只到題目。
3. 學生版的 AI 生成圖要不要標「AI 生成」？預設品質用 `medium` 嗎？每位老師每月要不要設張數上限？
4. 需要真實照片的位置（地圖、地標、歷史），要接 Openverse 或 Wikimedia 搜圖，還是只讓老師上傳？
5. 正式站的文字模型要不要換成 Claude？建議先用 10 份範例實測再決定。合規要在上線前確認規格與同意書。
6. 註記和採用／放棄的紀錄要不要當研究資料分析？要的話，同意書要寫到。

**問國網中心**

1. 閘道上的 `gemma-4-31B-it` 有沒有開 tool calling（`--enable-auto-tool-choice --tool-call-parser gemma4`，以及 vLLM 版本）？
2. 能不能送圖片給 `gemma-4-31B-it`（`image_url`）？可以的話，看範例圖這一步可以留在國內。
3. 有沒有產圖模型（例如 FLUX、SDXL、Qwen-Image），或有沒有計畫提供？計費和配額怎麼算？
4. 能不能針對單一請求關掉回應快取？
5. 同一個金鑰的並行數與速率上限是多少？逐區呼叫會讓請求數變多。

## 參考資料

- OpenAI：
  - [Image generation guide](https://developers.openai.com/api/docs/guides/image-generation)
  - [Images API reference](https://developers.openai.com/api/reference/resources/images/methods/generate)
  - [Pricing](https://developers.openai.com/api/docs/pricing)
  - 模型頁：[gpt-image-2](https://developers.openai.com/api/docs/models/gpt-image-2)、[gpt-image-2.6-flare](https://developers.openai.com/api/docs/models/gpt-image-2.6-flare)、[gpt-image-1-mini](https://developers.openai.com/api/docs/models/gpt-image-1-mini)
  - [Your data](https://developers.openai.com/api/docs/guides/your-data)
  - [API Organization Verification](https://help.openai.com/en/articles/10910291-api-organization-verification)（抓取回 403；相關描述取自官方指南與[社群討論](https://community.openai.com/t/gpt-images-1-403-what-are-the-conditions-for-requesting-this-model-gpt-image-1/1245846)）
  - [Codex review](https://developers.openai.com/codex/app/review)
  - [ChatGPT canvas](https://help.openai.com/en/articles/9930697-what-is-the-canvas-feature-in-chatgpt-and-how-do-i-use-it)
- Anthropic：[Pricing](https://platform.claude.com/docs/en/about-claude/pricing)、[Vision](https://platform.claude.com/docs/en/build-with-claude/vision)
- Google：[Gemini in Docs](https://support.google.com/docs/answer/13951448)、[Gemini API pricing](https://aistudio.google.com/docs/pricing)（只作比較用：Gemini 圖片模型每張約 $0.034–0.039）
- vLLM：[Tool calling](https://docs.vllm.ai/en/v0.25.0/features/tool_calling/)、[Gemma 4 31B recipe](https://recipes.vllm.ai/Google/gemma-4-31B-it)、[gemma4 tool parser](https://docs.vllm.ai/en/stable/api/vllm/tool_parsers/gemma4_tool_parser/)、[PR #38847](https://github.com/vllm-project/vllm/pull/38847)
- MagicSchool：[Studio Mode](https://help.magicschool.ai/en/articles/13457357-studio-mode)、[What's new（April 2026）](https://magicschool.ai/blog-posts/whats-new-april-2026)
- SchoolAI：[How do I edit part of a document](https://help.schoolai.com/how-do-i-edit-part-of-a-document)
- Wikimedia：[CommonsMetadata（授權欄位）](https://www.mediawiki.org/wiki/Extension:CommonsMetadata/en)
