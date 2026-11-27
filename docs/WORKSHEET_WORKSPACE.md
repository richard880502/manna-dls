# 學習單工作區（左邊直接編輯、右邊跟 Manna 對話）

狀態：2026-11-25 定案方向（使用者）。取代備課頁左邊的「預覽｜編輯」切換。

## 1. 老師看到的

- **右邊**：跟 Manna 對話。每一輪改了學習單，就寫成**新的一版**；對話卡片寫「產生了第 N 版」，點了打開版本紀錄並停在那一版。
- **左邊**：永遠是可以直接動手改的學習單（點字就改、每區工具），顯示最新的版本。Manna 產生中先顯示進度，好了就換成編輯器（不跳動）。Manna 這一輪在跑時左邊唯讀，送出對話前先存好手動修改（#115 的規則照舊）。
- **左邊只留學習單本身和一排精簡工具列**；小功能一律收進彈出式互動：

| 工具列 | 形式 | 內容 |
|---|---|---|
| 學生版｜教師版 | 分段按鈕（常駐） | — |
| 版本（「第 N 版 ▾」） | 選單 | 為這一版命名…（小對話框，名稱選填，預設「版本 11/25 17:30」）、版本紀錄（對話框：列表、預覽、還原；名字標示與篩選） |
| 顯示 | Popover | 顯示學習目標、Key Vocabulary 放在學習單最後、Key Vocabulary 的字義用中文（後兩項只管 Key Vocabulary 表，不動各區的 Word bank；學習單沒有 Key Vocabulary 時這兩項關閉並說明原因） |
| 詞彙檢查 | 大對話框 | 原 `/worksheets/:id/check` 的內容（分布、超出級別的字與處理、限制）；處理後關閉時學習單更新 |
| 預覽 | 大對話框 | 列印樣式預覽（伺服器投影＝匯出的樣子，可切學生版／教師版與各版本），放在匯出旁邊 |
| 匯出 | 選單 | DOCX／PDF，學生版／教師版 |
| ⓘ | Popover | 原「Manna 已帶入的資訊」與「這次由 Manna 決定…／作答方式…／份量…」說明 |
| 更多… | 選單 | 在完整編輯頁開啟（備課頁）、調整設定（完整編輯頁）；兩者都沒有時不顯示 |

- 每區工具：改這一區、加註記（常駐）；更多…：重新產生、修改歷程、復原上一版、上移／下移、關掉這一區、刪除、複製、有幫助／不合用。
- 窄螢幕（分頁）：「學習單」分頁就是同一個工作區。

工具列分兩組：左邊是看與編輯（學生版｜教師版、顯示、詞彙檢查、ⓘ、更多…），右邊是保存與輸出（已自動儲存的時間、第 N 版 ▾、匯出）；工具按鈕平常不加框，只有分段按鈕和匯出有框。工具列、Bloom 摘要和學習單紙張同寬（`--sheet-w`），備課頁的學習單欄比紙張寬時（對話收起、寬螢幕）整欄置中。

## 2. 有名字的版本（原「快照」）

- 自動儲存每次都寫一版（`cause = 'edit'`），版本紀錄會很長。老師可以「為這一版命名」，作為可以回去的重點。名字只是版本上的標記，不是另一種存檔，所以介面上不再叫「快照」（內部的表、端點、研究事件仍叫 snapshot）。
- 資料：`worksheet_snapshots (id, worksheet_id, version_no, name, created_at)`（migration 0058），一個版本可以有一個名字；移除名稱只是拿掉標記，版本本身不刪（版本是 append-only）。
- API：`POST /api/worksheets/{id}/snapshots { name?, base_version_no }`（先 flush，再標記最新一版；`base_version_no` 過期 409）、`PATCH …/snapshots/{snapshot_id} { name }`、`DELETE …/snapshots/{snapshot_id}`；`GET /versions` 的每一列多 `snapshot: { id, name } | null`。
- 版本紀錄：有名字的版本顯示名字；「只看有名字的版本」篩選；連續的「手動編輯」收合成一組（「手動編輯 ×12，17:02–17:30」），展開才看到每一版。
- 研究紀錄：`worksheet_snapshot_saved`／`_renamed`／`_removed` `{worksheet_id, version_no, named: bool}`（不記名稱文字）。

## 3. 程式結構（收斂）

```
components/worksheet/workspace/
  WorksheetWorkspace.tsx   工作區：useWorksheet + 工具列 + 編輯器 + 對話框們（備課頁左邊與 /worksheets/:id 共用）
  WorkspaceToolbar.tsx     第 1 節的工具列
  DisplayPopover.tsx       顯示設定
  InfoPopover.tsx          ⓘ 說明
  PrintPreviewDialog.tsx   列印樣式預覽（SheetPreview 唯讀）
  VocabCheckDialog.tsx     詞彙檢查（內容抽自 WorksheetCheckPage，頁面改為包同一個元件）
  VersionMenu.tsx          「第 N 版 ▾」：為這一版命名…、版本紀錄
  NameVersionDialog.tsx    為這一版命名
  VersionHistory.tsx       （搬入）版本紀錄＋名字
```

- `ResultView` 只負責「學習單本體的編輯」（區塊、題目、每區工具），工具列與說明移到 `WorkspaceToolbar`／Popover；`embedded`、`frozen` 照舊。
- `/worksheets/:id`（完整編輯頁）＝ 教材欄 ＋ `WorksheetWorkspace`；調整設定（OptionsView）仍在這一頁。
- 備課頁左邊＝ `WorksheetWorkspace`（產生中顯示 `WorksheetCard` 的進度，好了換成工作區）。拿掉 `SheetModeToggle`、`sheetMode` 儲存。
- `SheetPreview`（伺服器投影）只留做唯讀呈現：版本預覽、列印樣式預覽、對話卡片。預覽獨有的功能搬進編輯器：每區評分（`item_index`＝區塊位置，資料格式不變）、Manna 改過的區塊標示、「這一輪改了」的捲動目標（區塊元素 id 不變）。
- 已完成：首頁與 `/create` 共用的開始流程抽成 `components/start/useStartFlow.tsx`（忙碌／錯誤、OCR 辨識中的拒絕、擬目標）。
