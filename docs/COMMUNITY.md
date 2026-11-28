# 社群（第一期）

老師可以選擇把自己的學習單分享出來，其他老師可以瀏覽、收藏，或「拿來改」成自己的副本。
這是研究平台：老師只用研究代號識別，過去老師看不到彼此的任何資料，所以隱私與研究紀錄比功能完整更重要。
端點契約見 [API.md](API.md)「社群」。

## 研究開關

- `app_settings.community_enabled`，預設**關閉**，只有主持人（`pi`）能改：`GET`／`PUT /api/admin/community-settings`，
  後台管理 →「模型與研究設定」有一張「社群」卡片，「總覽」有一列。
- 每次改動寫一筆 `usage_logs`（`setting_changed`，`key`、`from`、`to`），和教師長期記憶的開關一樣。
- 關閉時：所有社群端點回 503「社群功能目前沒有開放。」（包括分享、取消分享），側邊欄沒有「社群」，
  `/community` 會導回首頁，學習單頁也不再顯示「改編自」。`GET /api/config` 的 `community_enabled` 帶著開關狀態，網頁靠它決定要不要顯示。
- 關閉**不會**清掉分享的設定：老師分享的狀態與收藏都留著，只是看不到；再開啟就回來。
  需要真的撤回時，由老師自己取消分享。

## 分享什麼

- 分享是老師對**自己的一份學習單**做的選擇（只有擁有者；別人的學習單回 404），預設不公開。
  學習單要已經產生內容才能分享（`generated`）。
- 分享時老師選：名稱怎麼顯示（`code` 顯示研究代號，`anonymous` 顯示「一位老師」；沒有預設，必須選）、
  是否允許改編（預設允許）。網頁在打開開關前會先跳出說明視窗（別人看得到什麼、看不到什麼、可隨時撤回）。
- 分享的是這份學習單的**目前版本**：老師之後修改，別人看到的也跟著變。取消分享立即生效（每一次讀取都檢查 `shared_at`）。
- 已經做出的副本留在各自的老師那裡；取消分享不會收回。

## 別人能看到什麼

探索、收藏、預覽只回這些：編號、標題、顯示名稱（依 `share_name`）、分享時間、收藏數與改編數、
`is_mine`、`saved`、`allow_remix`、學習單類型（`output`）與版本（`levels`）、封面或預覽的**學生版學習單**（印在學習單上的內容）。

- 預覽是學生版的投影：沒有參考答案、依據、教師備註、評分說明；教材只出現在「該版本自己會印的原文」。
  不回教材庫的資料、`material`、段落、教師的要求、備課對話、記憶、AI 呼叫紀錄，也不回任何擁有者的內部編號
  （`teacher_id`、`lesson_id`、`class_id`、`prep_session_id`、`interaction_id`、`material_id`…）。
- 匿名分享時，研究代號在查詢裡就不會被選出來（`CASE WHEN share_name = 'code'`），不只是回應時遮掉。
- 圖片：預覽裡的圖用 `GET /api/community/worksheets/{id}/images/{image_id}` 讀，只有分享中、該位置**目前**的圖才讀得到。
- 封面是學生版開頭的 2 個區塊、每區 3 題（不含圖與答案），網頁用真正的學習單元件縮小畫出來，不產生圖片。
- 收藏與改編只有**次數**，沒有誰：擁有者看不到誰收藏、誰改編。

## 收藏

收藏的是一份分享中的學習單（不能收藏自己的）。收藏記在 `worksheet_saves(teacher_id, worksheet_id)`，
取消分享或刪除後，那一筆從收藏清單消失（清單回 `unavailable` 數量），重新分享時又會回來。

首頁「從範本挑選」的「我的收藏」會列出仍在分享的收藏。選用時呼叫
`POST /api/community/worksheets/{id}/template`，把分享出去的最新學生版區塊結構做成呼叫者自己的 ready
版型；端點只接受呼叫者已收藏且仍在分享的學習單。轉換只讀分享學習單本身已公開的區塊、題型、指示與提示框，
不讀、不複製原作者私人版型的 style notes、原檔或教材庫資料。

## 拿來改

`POST /api/community/worksheets/{id}/remix`：只有 `share_allow_remix` 為真、不是自己的、仍在分享中時，
替呼叫者做一份**私人副本**（不公開，`remixed_from` 指回原件）。

副本帶走：標題、教材文字（段落，沒有它副本無法重新產生或改寫）、學習單設定、最新版本的內容與圖片
（圖片檔共用同一個儲存物件，各自有自己的資料列）。

副本**不帶**：課例、班級、備課對話、版型、課例 SOP 版本、教材庫來源（`material_id`、教材來源備註）、
教師的要求、教師寫的備課想法（brief）、註記／建議／修改歷程，以及內容裡指向原作者 SOP 與版型的編號。
檔名重新依教材產生。副本的版本紀錄從 1 開始。

副本的學習單頁顯示「改編自「標題」（顯示名稱）」，只在原件還在分享、開關也開著時顯示；之後原件取消分享，副本就不再提它。
原件只累計「被改編幾次」（含副本之後被刪除的），沒有名單。

> 取捨：允許改編就代表改編的老師會拿到教材文字與參考答案；分享說明視窗有明講。不想給就把「允許改編」關掉。

## 研究紀錄（`usage_logs`）

只記編號、布林值與固定標籤，不記任何自由文字。事件的 `teacher_id` 是做這個動作的老師。

| 事件 | 時機 | payload |
|---|---|---|
| `setting_changed` | 主持人改開關 | `key` = `community_enabled`、`from`、`to` |
| `community_share` | 分享，或分享中改選項 | `worksheet_id`、`name`（`code`／`anonymous`）、`allow_remix`、`update`（是改選項） |
| `community_unshare` | 取消分享（沒有分享時不記） | `worksheet_id` |
| `community_view` | 打開預覽（含擁有者看自己的） | `worksheet_id`、`own`、`level` |
| `community_save` | 收藏（已收藏不重記） | `worksheet_id` |
| `community_unsave` | 取消收藏（沒有收藏不記） | `worksheet_id` |
| `community_remix` | 做出副本 | `worksheet_id`（原件）、`copy_id` |
| `community_template_created` | 把收藏做成自己的版型 | `worksheet_id`（原件）、`template_id` |

瀏覽探索清單與搜尋目前**不**記錄。副本另外不寫 `worksheet_created`。

## 資料表（`0052_community.sql`）

`worksheets` 加 `shared_at`、`share_name`（`code`／`anonymous`，與 `shared_at` 同時有或同時沒有）、
`share_allow_remix`（預設 true）、`remixed_from`（`ON DELETE SET NULL`）；新表 `worksheet_saves`。

## 第一期不做

關係圖、課堂註記、追蹤老師、老師頁面、推薦、暱稱、收藏夾、縮圖圖片。
