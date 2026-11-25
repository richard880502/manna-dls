# 備課對話：工具式對話（prep chat）設計

狀態：2026-11-25 已實作（後端：`api/src/prep/chat/`、Migration `0056_prep_chat.sql`；介面：`web/`）。使用者定案：「改成工具式對話」；國網 gemma 不再使用，文字模型只用 OpenAI。
取代備課對話底部的「改學習單（revise）／問 Manna（ask）」兩種模式。做學習單、規劃活動、評量、資源等按鈕輪次不變。

## 1. 老師看到的

- **並排版面**（使用者 11/25）：寬螢幕左邊是目前的學習單（預覽、學生版／教師版、版本、匯出、看答案、圖，自己捲動、固定在畫面上），右邊是對話（輪次與底部輸入框，自己捲動）；中間的分隔線可以拖曳調整寬度（學習單預設 60%，學習單至少 32rem、對話至少 20rem；雙擊或 Enter 回到預設，方向鍵每次 2%、Shift 10%；記在這台瀏覽器）。對話裡的學習單輪次只顯示精簡卡片，不再整份嵌在對話裡。一輪改了學習單後，左邊更新到新版本並短暫標出改過的區塊；點「這一輪改了」的項目會捲到左邊那一區。窄螢幕改成上方「對話｜學習單」兩個分頁。
- 底部只有一個輸入框，沒有模式切換。對話裡有學習單時，輸入框上方顯示「目前的學習單：<標題>」（沿用現在的目標學習單選擇）。
- 老師說什麼都可以：問問題、要求修改、或兩者一起（「把 C 區第 2 題改成比較表，順便把圖重畫成女生」）。
- 這一輪執行中，對話裡顯示進度列，一步一行，例如「讀 C 區」「修改 C 區」「重畫圖」，沿用 `PrepTurn.progress`（RunProgress）。
- 結束後一張卡片：
  - Manna 的回答（段落，可以長，最多約 1,500 字；可附最多 3 個追問建議，沿用 ask 的 follow_ups 按鈕）。
  - 有改學習單時：「這一輪改了」清單，每列一個區塊或一張圖，寫出改了什麼（標題、指示、第幾題、表格、鷹架、字詞表、圖），可點過去看；一個「復原這一輪」按鈕。
  - 有沒通過檢查而沒改成的，列出「沒有改成：B 區（原因）」。
  - 學習單預覽照現在的方式更新到新版本。
- 有幫助／不合用沿用現有機制。

## 2. 一輪的流程（伺服器）

新輪次種類 `chat`（`prep_turns.kind = 'chat'`），由 `POST /api/prep-sessions/{id}/turns { kind: "chat", request, image_ids?, worksheet_id? }` 建立（沒寫 `kind` 也是 chat），背景執行，前端照舊輪詢。

- `image_ids`：照舊，從 `POST /api/prep-sessions/{id}/images` 上傳的圖片 id（最多 2 張）。
- `worksheet_id`：只有老師選了「不是這個對話最新的學習單」時才送；沒有時用這個對話最新的學習單（最近一個做出學習單的輪次）。必須是這個老師在這個對話裡做出來的學習單，否則 404。對話還沒有學習單時 chat 只能回答，不能修改（不提供工具）。這份學習單正在產生時也一樣只回答，並告訴模型這一輪不能修改。
- 對話裡的 `ask`／`revise` 不能再新建：`POST /turns` 與 `POST /prep-sessions` 收到這兩種 `kind` 回 400；舊輪次照舊載入與顯示。（設定 `PREP_LEGACY_TURNS=true` 可以暫時讓它們回來；測試舊流程用。）

一輪的步驟：

1. 組訊息（順序固定，OpenAI 自動提示快取才命得中）：
   1. system：`prep_chat` 提示詞（可在後台改版本）；老師已確認的習慣（照 `teacher_memory_context`，步驟名稱 `prep_chat`）附在最後。
   2. user（固定前綴）：班級特性、教材（加段落標記）、學習單目前內容的精簡版（每區：block_id、印出的字母、種類、標題、作答指示、每題的作答方式與題目、表格欄列、有沒有句型框架或提示、鷹架摘要、Key Vocabulary、圖的 slot_id 與狀態；沒有答案，答案與全文用 `read_block`）、圖片說明、作答方式清單、老師的習慣。
   3. user：先前的對話——最近 6 輪的 `{種類、老師說了什麼、結果的一行}`（chat 輪次是回答加上「改了：…」；每輪最多 700 字）。
   4. user：這一輪老師的話（與附圖）。
2. 呼叫模型（OpenAI chat completions：`tools`＝嚴格 JSON schema 的函式、`tool_choice: "auto"`、`parallel_tool_calls: false`；同一輪每次呼叫的 `response_format` 相同）。
3. 模型回 `tool_calls` → 伺服器逐一在**工作副本**上執行 → 以 `role: "tool"` 把結果（成功的摘要，或檢查不通過的原因）回給模型 → 再呼叫；後面的呼叫只在後面追加訊息。直到模型不再呼叫工具：它的最後一則訊息就是回答。
4. **回答怎麼產生**：最後一則訊息受嚴格 JSON schema 約束（`{ paragraphs[], follow_ups[] }`，同 `prep_ask`；schema 在每次呼叫都一樣，所以請求前綴不變，也不用多花一次呼叫）。伺服器把段落用空行接成 `answer`（去掉 Markdown 符號，最多 2500 字）；訊息不是這個 JSON 時（不應發生）就把全文當回答，追問為空——所以一輪不會因為最後一則訊息的格式而失敗。沒有文字時伺服器補一句（「已依你的要求處理…」「這次沒有改成…」「這一輪處理完了…」），`answer` 一定是字串。
5. 上限：一輪最多 8 次工具呼叫、最多 10 次模型呼叫、每次呼叫沿用 AI 設定的等待上限。模型想做第 9 次工具呼叫時，那次呼叫不執行（回「次數已用完」），再叫一次模型（`tool_choice: "none"`）請它用文字收尾；回答後面加一段說明（「這一輪的工具次數已經用完…」）。
6. 所有修改在記憶體裡的**工作副本**上進行；整輪結束時**一次**寫成學習單的一個新版本（`cause = "revise"`，`source = { cause: "prep_turn", turn_id, block_ids }`），每個被改的區塊先放進 `worksheet_block_history`（附 `after_block` 與 `version_no`），沒有任何修改就不寫版本。
7. 寫入前檢查：學習單開始產生、被刪除、或最新版本不是這一輪開始時的那一版（別處改了）→ 這一輪 `failed`（「這份學習單在 Manna 處理的時候被改過了…」），什麼都不套用，圖也不重畫。
8. 圖的重畫是背景工作（`images::regenerate`）：整輪寫入版本**之後**才送出；每份學習單的重畫上限照舊（不夠就在工具階段拒絕，列在 `not_applied`）。
9. 不是 OpenAI 模型（國網、Anthropic 示範）：這一輪 `failed`，「這個功能需要 OpenAI 模型。…」，不呼叫模型。AI 關閉時照舊的「這個環境沒有開啟 AI 協助」。

進度（`PrepTurn.progress`，沿用 RunProgress）：一開始「讀學習單」；每次工具呼叫一步（「讀 C 區」「修改 C 區」「改 C 區的鷹架」「改 Key Vocabulary」「重畫 C 區的圖」「新增一區」「關閉 C 區」「移動 C 區」，失敗的步驟標成 failed）；最後「整理回答」。

## 3. 工具

每個工具的參數用嚴格 JSON schema（不改的參數給 `null`）；伺服器檢查一律沿用現有的檢查程式，失敗回傳原因讓模型修正。**同一個工具在同一區最多失敗 3 次（第一次加 2 次重試）**，第 4 次起不執行、回「不再重試」；沒有成功過的失敗列在 `not_applied`。

| 工具 | 參數 | 做什麼 | 檢查沿用 |
|---|---|---|---|
| `read_block` | `block_id` | 回傳該區完整內容（題目含答案、表格、鷹架全文、字詞表、Glossary、圖格描述） | — |
| `edit_block` | `block_id`, `title?`, `instructions?`, `items?`（完整取代這一區的題目；每題 `answer_format, prompt, options, answer, table?{columns, row_labels}`） | 改標題／作答指示／題目 | `check_block_change`（`evaluate_section`：固定作答方式、題幹不含答案、音節、題數、表格欄列與標題檢查）；每題保留原本的 Bloom、提示、versions，句型框架與表格只在題目沒變時保留 |
| `edit_scaffolds` | `block_id`, `steps?`（程序鷹架的步驟）, `frames?`（`[{item_no, frame}]`，`""` 拿掉）, `hints?`（`[{item_no, hint}]`）, `word_bank?{words, lang, show_meaning[]}` | 改步驟鷹架、逐題句型框架與提示、Word bank | 步驟 1–12 步；`item_frame`／`frame_format`（只有簡答題與段落寫作、英文、要有空格、不含答案）；`item_hint`；Word bank 的字要是英文字，詞性由詞彙表補，沒有字義的字不能勾選印出字義 |
| `edit_key_vocabulary` | `block_id?`（預設有 Key Vocabulary 的那一區）, `add[]{word, pos, definition, meaning_zh}`, `remove[]`, `language?`（`en`｜`zh`） | 增減 Key Vocabulary、切換字義語言 | 字要出現在教材或這一區；詞性經 `key_vocab::normalise_pos`／`resolve_pos`（詞彙表只有一個詞性時以詞彙表為準）；級別一律由程式查詞彙表；指示用語不能拿掉；全部拿掉時保留 `vocabulary_removed` |
| `redraw_picture` | `slot_id`, `note` | 依老師的要求重畫一張圖（記為重畫，`note` 當附註） | 圖片要開、有儲存空間、這張圖已畫過且沒在繪製中；每份學習單的重畫上限（這一輪已排的也算）；`checked_hint`（≤200 字、沒有網址） |
| `add_block` | `after_block_id?`, `title`, `instructions`, `items` | 新增一個活動區塊（`null`＝放最前面） | 同 `edit_block`；最多 60 區 |
| `remove_block` | `block_id` | 關閉（`enabled = false`）一個區塊，不真的刪 | 至少留一個區塊 |
| `move_block` | `block_id`, `after_block_id?` | 調整順序（區塊字母跟著重排） | — |

不提供：改教材原文、改學習目標、產生整份新學習單（那些仍用按鈕輪次）。

## 4. 結果與 API

`PrepTurn.kind` 新增 `"chat"`；`result`（`changes`、`not_applied`、`follow_ups` 一定是陣列，`answer` 一定是字串）：

```ts
interface PrepChatResult {
  answer: string;                 // 段落以 \n\n 分隔
  follow_ups: string[];           // ≤3
  changes: Array<{                // 由伺服器比對「改之前／改之後」的學習單產生，不是模型自己說的
    target: "block" | "picture" | "vocabulary";
    block_id: string | null;
    label: string;                // 「C 區」「C 區的圖」「Key Vocabulary」
    summary: string;              // 中文一句：改了標題、第 2 題改成表格整理（欄：…）、新增 2 題…
  }>;
  not_applied: Array<{ label: string; reason: string }>;
  version_no: number | null;      // 這一輪寫成的版本；沒有修改（或只重畫圖）是 null
  undoable: boolean;              // 這一輪的修改目前還能整輪復原（讀取時依學習單現況計算）
  tool_calls: number;             // 執行的工具呼叫數（含失敗的；不含被上限擋掉的）
}
```

- `POST /api/prep-sessions/{id}/turns/{turn_id}/undo`（只有擁有者；別人的對話 404）：把這一輪改過的每個區塊還原，寫成**一個**新版本（`cause = "undo"`），標記區塊歷程為已復原，把這一輪的 `result.undoable` 設成 `false`，記 `prep_chat_undo`。回傳更新後的 `PrepTurn`（任何 2xx 都是成功）。這一輪新增的區塊會被移除、關閉的會重新開啟、調整過的順序會回復。
- 409（標準錯誤格式 `{ "error": { "message": "…" } }`）：這一輪沒改任何東西或已經復原過（「這一輪沒有可以復原的修改。」）；某個被改的區塊之後又改過（「C 區在這一輪之後又改過了，所以不能整輪復原。可以到那一區單獨復原。」）；區塊順序之後又調整過；學習單正在產生。
- 圖的重畫不復原（已經產生的圖保留，可在圖的歷程換回）。
- 舊的 `ask`、`revise` 輪次照舊顯示（唯讀），不再能新建（見第 2 節）。
- 老師在 chat 訊息裡寫的長期習慣（「之後…」）照舊由 `memory::detect` 偵測，建議的 `source = 'prep_turn'`、`prep_turn_id` 是這一輪的 id（同 revise）。

## 5. 研究紀錄

- 每一次模型呼叫一列 `ai_interactions`（`module = prep_chat`，`attempt` = 這一輪的第幾次呼叫）。`input_snapshot`：`inputs`（老師這一輪的話照記；教材、學習單衍生內容、先前的對話只記字數與雜湊）、`prompt`（key、版本、已遮蔽的系統與使用者文字）、`messages`（每則訊息的角色、字數、雜湊；助手的工具呼叫記名稱與參數字數）、`tools`、`request`、`teacher_memory`。`output`：工具呼叫（名稱與參數雜湊）或最後的回答。`usage`（Migration 0056 新增的欄位）：`{ prompt_tokens, completion_tokens, cached_tokens }`，`cached_tokens` 是 OpenAI 回報的 `usage.prompt_tokens_details.cached_tokens`。
- 每一次工具呼叫記 `usage_logs` event `prep_chat_tool`：`{turn_id, tool, block_id, ok, reason_code}`（不記老師原文）。`reason_code`：`ok`、`bad_arguments`、`unknown_block`、`check_failed`、`retry_limit`、`limit`、`unavailable`。
- 輪次結束記 `prep_chat_turn`：`{turn_id, tool_calls, model_calls, changed, changes, not_applied, undone: false}`；復原記 `prep_chat_undo`：`{turn_id, worksheet_id, version_no, undone_version_no, blocks}`。
- 「問問題」與「改學習單」由 `changed` 判定，不靠模式。

## 6. 模型與費用

- 只用 OpenAI（AI 設定的文字模型）；國網與 Anthropic 示範不支援（第 2 節第 9 點）。
- **API：Responses API**（`POST https://api.openai.com/v1/responses`），不是 chat completions。2026-11-25 正式站第一輪就遇到 HTTP 400：`gpt-5.6-sol` 在 `/v1/chat/completions` 不接受「函式工具＋reasoning effort」（要用 `/v1/responses`，或把 effort 設成 `none`）。臨時修正（#109）曾在有工具時送 `reasoning_effort: "none"`；改走 Responses API 後這個限制不再存在，推理強度照 AI 設定（low／medium／high）送出。
- 請求（`api/src/ai/chat.rs` `build_body`）：`input` 依序是 developer 指示、固定前綴、先前對話、這一輪的要求與圖片（`input_text`／`input_image`）；`tools`（`type: function`、`strict: true`）、`tool_choice`（`auto`／最後一次 `none`）、`parallel_tool_calls: false`、`reasoning: {effort}`、`text.format`（嚴格 `json_schema`，最終回答）、`max_output_tokens`（回答上限＋推理空間 `REASONING_ROOM`）。
- **不存檔**：`store: false`，教材與學習單內容不留在 OpenAI 那邊。因為不存檔，推理內容用 `include: ["reasoning.encrypted_content"]` 取回，同一輪的下一次請求原樣送回去（`reasoning`、`function_call` 等輸出項目原封不動放在 `ChatMessage::Assistant.replay`，後面接 `function_call_output`）。只有要繼續呼叫工具的那一步需要帶回；最終回答不用。
- 回應：`function_call` 項目成為工具呼叫（`call_id`、`name`、`arguments`）；最終文字取 `message` 項目的 `output_text`（沒有就用頂層 `output_text`）；`status: incomplete` 且原因為 `max_output_tokens` 走與「回答被截斷」相同的錯誤路徑，`content_filter` 與 `refusal` 視為拒答。HTTP 錯誤、408／504、非 JSON 的對應與以前相同。
- 提示快取：固定前綴放最前面，同一輪內的後續呼叫只在後面追加（`tools` 與 `text.format` 每次都一樣）；另送 `prompt_cache_key = prep-chat-<對話 id>`（穩定、不含個資），讓同一場備課的呼叫落在同一份快取。`ai_interactions.usage` 記 `prompt_tokens`（`input_tokens`）、`completion_tokens`（`output_tokens`）、`cached_tokens`（`input_tokens_details.cached_tokens`）與 `reasoning_tokens`（`output_tokens_details.reasoning_tokens`，已含在 completion 裡）。
- 費用估計：一般一輪 1–3 次模型呼叫；上限 10 次。

## 7. 測試用假模型

`AI_PROVIDER=fake`：依**這一輪的請求**（最後一則 user 訊息；前面輪次的標記不算）裡的標記腳本化工具呼叫（細節在 `api/src/prep/fake.rs` 開頭）：`FAKE_CHAT_ASK`（不呼叫工具）、`FAKE_CHAT_EDIT`（`edit_block` 改第一個有題目的區塊）、`FAKE_CHAT_BAD_EDIT`（先給不合格的作答方式，收到原因後修正）、`FAKE_CHAT_REFUSED`（一直給不合格的，直到伺服器停止）、`FAKE_CHAT_LOOP`（一直 `read_block`，測上限）、`FAKE_CHAT_TABLE`（第一題改成表格整理）、`FAKE_CHAT_PICTURE`／`FAKE_CHAT_EDIT_PICTURE`（重畫第一張圖／先改再重畫）、`FAKE_CHAT_SCAFFOLD`、`FAKE_CHAT_VOCAB`、`FAKE_CHAT_BLOCKS`（新增再移動區塊）、`FAKE_CHAT_PACED`（每次呼叫前停 400 ms，測試趁機改學習單）。假模型回報的 `usage` 的 `cached_tokens` 是第一次工具回合之前的訊息量，所以第二次呼叫起大於 0。

## 8. 實作分工

- 後端：`api/src/ai/chat.rs`（訊息、工具規格、OpenAI 的 `tools`／`tool_calls`／`role: "tool"`／`cached_tokens`）、`api/src/prep/chat/`（`mod.rs` 流程、一次寫版本、復原；`tools.rs` 工作副本與工具；`view.rs` 精簡版與變更清單）、`prep::fake::chat`、`0056_prep_chat.sql`（`chat` 種類、`prep_turns.chat_undo`、`ai_interactions.usage`、`prep_chat` 第 1 版）、`routes/prep.rs`（建立輪次、復原）。
- 前端：單一輸入框（沒有改學習單／問 Manna 切換）、chat 輪次卡片（回答、追問、改了什麼、沒改成、復原這一輪）、執行中進度、舊 ask／revise 卡片照舊顯示。
