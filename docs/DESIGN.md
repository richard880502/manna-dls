# Design: Manna DLS

The visual system of the web app (`web/`). Every screen reads it; change it on purpose. The source of truth for values is `web/src/styles/tokens.css`; this file says what each token is for and how the parts are put together. The direction and the reasons are in [`DESIGN_OVERHAUL.md`](DESIGN_OVERHAUL.md); the approved mockups are in [`design/overhaul/`](design/overhaul/index.html), and before/after screenshots of what is built so far are in [`design/overhaul/implemented/`](design/overhaul/implemented/).

**Status:** overhaul steps 1 (shell and list pages) and 2 (the seven-step lesson workspace, direction A) are in the app; the CoRe editor, 課後補寫, preferences and admin keep their layout and take the new tokens, heads and frameless panels. The simplification of 2026-11 made the teacher's path 班級 → 單元 → 課例（七步）→ 學習單 the only one on screen: the lesson page is the seven steps, the unit page is its lessons, class traits live on the class page only, and materials have their own destination.

## System

- Genre: editorial, at app scope (screens someone works in, not pages someone reads once)
- Shape: a 56px top bar with three destinations (班級, 學習單, 教材); pages are a column of sections
- Vibe: "a bright, plain product UI with one blue and one gold": the chrome has no colour, blue means "you can act here", gold marks what the AI contributed, and everything else colourful comes from the content
- Axes: white page / bold sans heads / one-blue accent. Posture: restrained and flat, plus one gold with one job (light only)
- Modes: light only (see "Not done")

## Principles (from the overhaul)

1. **One view, one thing.** One filled button per view; everything else is an outline button or a text link.
2. **Space, type size and hairlines part sections, not boxes.** Only three things are filled surfaces: what Manna suggests (a barely warm wash with a thin gold edge), source material and worksheets (the print sheet, from step 2).
3. **Three voices.** The teacher's own words and actions are the page ink, and the one blue (#1f5eff) marks what she can act on (buttons, links, selected filters, focus). Whatever Manna (the AI) adds carries the gold, quietly (a thin edge or a small mark, not a big wash), and its sources. Source material is print: serif on the lightest sheet, paragraph numbers in the margin (step 2).
4. **Nothing blocks.** No required fields; every step can be skipped; what the AI produces is only written in when the teacher says so.
5. **The teacher's words.** No 「版本」「Phase」「RAG」「選用」 on controls; navigation and buttons are in Chinese. CoRe is called 「單元想法」, with CoRe in small print.
6. **The class is context, not a field.** The class is the one the teacher came from; it appears once, in the breadcrumb. It is asked for in exactly one place: starting a lesson from the library when several classes use its unit.

## Tokens

### Colour

| Token | Value | Job |
| --- | --- | --- |
| `--color-paper` | `#ffffff` | the page |
| `--color-paper-2` / `-3` | `#f7f7f7` / `#ededed` | neutral wells, quiet tags, the selected sidebar pill / hover, progress tracks |
| `--color-field`, `--color-sheet` | `#ffffff` | inputs; the print sheet (material, worksheets) |
| `--color-ink` | `#191919` | text and heads (17.6:1 on paper) |
| `--color-ink-2` | `#3d3d3d` | secondary text, lead paragraphs (10.9:1) |
| `--color-muted` | `#696969` | meta lines, hints, crumbs (5.5:1 on paper, 5.1:1 on paper-2, 4.7:1 on paper-3) |
| `--color-field-edge` | `#8c8c8c` | input and outline-button edges (3.4:1 on white) |
| `--color-rule` / `-strong` | `#e8e8e8` / `#d0d0d0` | hairlines between rows / the rule under a focal block or around an open form |
| `--color-accent` (+ `-hover`, `-press`, `-ink`, `-text`) | `#1f5eff` (hover `#1a4fd6`, press `#1541b3`; `-text` is `#1a52e0`) | the one blue, "you can act here": the filled action, links, selected filters and tabs (white on blue 5.1:1; text 6.3:1 on white, 5.5:1 on the wash) |
| `--color-accent-wash` / `-wash-2` / `-rule` | `#eaf0ff` / `#d6e2ff` / `#b9ccff` | selected tiles / pressed / link underline |
| `--color-focus` | `#1f5eff` | the focus ring (5.1:1 on paper) |
| `--color-star` | `#d4a72c` | Manna's star |
| `--color-manna-wash` / `-wash-2` / `-rule` / `-ink` | `#fffaec` / `#fbf0cc` / `#dcc16f` / `#6b4a0c` | everything Manna produced, and the button that asks her: a faint wash, a thin gold edge, small gold marks (ink on wash 7.7:1 and 7.1:1) |
| `--color-success`, `--color-warn-ink`, `--color-error` (+ washes) | | 順利 and saved / 卡住 and the demo banner / errors (each 5.5:1 or better on its wash) |

Rules: no colour literal outside `tokens.css`; never pure black or white as a surface; grey text never sits on a coloured surface; red and green never carry meaning alone (every state also has a word or an icon).

### Type

Self-hosted with `@fontsource-variable` (pinned, `font-display: swap`, sliced by Unicode range). No request leaves for a font CDN.

| Role | Face | Size / weight |
| --- | --- | --- |
| Heads (h1-h4, `--font-display` is the sans family), row titles, Q numbers, the lesson clock | Noto Sans TC, 700, letter-spacing `--tracking-display` (-0.025em) | 700 on h1-h4, 600 elsewhere |
| Everything else | Noto Sans TC, `--font-body` | 400, 600 for labels and buttons |
| Latin names beside CJK (CoRe), and from step 2 material and worksheets | Georgia, `--font-print` | 400 |
| Machine identifiers only (codes, SHA, prompt templates) | system mono, `--font-mono` | 400 |

Scale (fixed): `--text-xs` 13 (tags) · `--text-sm` 14 (meta, hints) · `--text-md` 16 · `--text-lg` 18 (row titles, the big button) · `--text-xl` 21 (section heads) · `--text-2xl` 25 (page title on a phone) · `--text-3xl` 30 (page title) · `--text-clock` 44.

Heads: `h1` is the page title (30px; 25px under 760px), one per page. `h2` is a section (21px). `h3` is a row title or a sub-block (18px). `h4` 16px.

CJK rules: body never under 16px; prose `line-height` 1.75, UI 1.55, heads 1.3; measure `--measure: 40em`; no tracking, no uppercase; `line-break: strict`, `text-autospace`; `text-wrap: balance` on heads.

### Space, shape, depth, motion

- **Space:** `--space-3xs` 2 … `--space-4xl` 96; sections of a page are `--space-section` apart (56, 40 on a phone). Gutters `--gutter` 40 / 16.
- **Radius:** `--radius-control` 8 (buttons, inputs, wells) · `--radius-card` 12 (dialogs) · `--radius-mark` 16 (the two in-class buttons) · `--radius-pill` (tags).
- **Depth:** space and hairlines. Shadows only on things that float: `--shadow-overlay` (the help sheet over a narrow screen) and `--shadow-dialog` (the choose-a-class dialog), over `--color-scrim`.
- **Motion:** state only. `--dur-micro` 120ms for colour, `--dur-short` 220ms for a chevron; only `transform`, `opacity` and colours move. Under `prefers-reduced-motion: reduce` everything ends at once.
- **Layers:** `--z-sticky` (top bar, bottom nav), `--z-sticky-nav` (demo banner), `--z-overlay` (dialogs, the leave confirm, the help sheet).

## Layout

- **Top bar** (`.top`, 56px, sticky under the demo banner): the star and 「Manna」, the three destinations (班級, 學習單, 教材), and at the right the account links (知識庫管理 for the project lead and knowledge-base managers, 後台管理 for the project lead, 「研究編號 · 備課偏好」, 登出). Under 760px the three destinations move to a fixed bar at the bottom (60px) and the research ID drops out of the top bar.
- A page that takes over the bar renders into its slot (`TopBarSlot`): during a lesson the bar shows only 「上課中 · 已同步」 and 「結束本節課」, and the bottom bar is gone, so nothing invites a stray tap.
- **Frames** (`.workspace`): list pages 1040px; working pages with two columns (lesson, 單元想法, 課後補寫, admin) 1200px; the in-class console 880px; single-column text pages 800px; 我的作品 1280px (`--container-gallery`: a wall of cards, at least 240px each; its list view stays 800px wide).
- **Breakpoints:** 760px (shell, rows, single column), 1100px (the lesson page's side column goes under the main one), plus the older 75rem / 60rem / 40rem inside screens not rebuilt yet.

## Components

- **Sidebar and home (nav redesign).** Every tool appears once and has one name (`TOOL_NAME`). The sidebar (`shell.css`) is 首頁, 我的作品 (`/prep`; also current for `/prep/:id` and `/worksheets…`), at most three 最近 sessions, a small 「教材與範本」 label over 我的教材, 平台教材 and 學習單範本, a folded 更多, then 備課偏好 and the account. It has no 建立 group: creating starts on 首頁, which is the current item on `/create/*`. The home page's composer (`.compose-card.is-quick`) is the same rounded box as the prep session's (`.prep-composer`): text on top, 附上 controls bottom-left, a round arrow bottom-right, the border and focus ring on the box. Under it three ways to prep, 「AI 幫我設計」 (the 完整學習單 tool), 「照我的範本設計」 (the 照我的學習單做 tool) and 「我想逐步設計」 (`HOME_TOOLS`, names in `HOME_NAME`, apart from `TOOL_NAME`), are one `radiogroup` of tiles (`.tool-tile`: icon, name, one-line blurb; three per row; arrow keys move and choose; chosen = ink frame plus wash, so colour is not the only cue). The other four tools (閱讀理解題, 差異化學習單, 課堂活動, 教學套件) have no tile: a quiet line (`.home-others`) says they are done in the conversation after the worksheet, or by writing them in the box (`toolFromWords` still picks them; the conversation's 接下來可以 chips and `/create/:tool` offer all six). `TOOLS` still lists all six; only the home page uses `HOME_TOOLS`. The tiles' names are the home page's own: the generator's heading, the 接下來可以 chips and 我的作品 keep the tools' names (`TOOL_NAME`), and the chip text the server recognises (`TOOL_CHIP`) is unchanged. 「我想逐步設計」 is a tile, not a generator tool (not in `TOOLS`, no `/create` page): with it chosen the box's text only names the lesson, the arrow opens a dialog (shared `Dialog`, guarded when dirty) where the teacher picks an existing unit or makes a new one and names the lesson, and the attached article goes in as step 1 (閱讀教材) of the new lesson at `/lessons/:id`. 我的作品 lists those lessons too (tag 「逐步設計」, no thumbnail, no 刪除: lessons cannot be deleted yet). With an article attached and no tile or tool word chosen, sending makes a 完整學習單 (it used to make 閱讀理解題, which no longer has a tile). Then one quiet link 「更多設定（班級、題數、Bloom、題型）」 to `/create/<tool>`. Bare tool labels (`TOOL_CHIP`) are only the request text sent when nothing was typed; they are never shown.
- **Buttons.** `primary-button`: filled accent, **one per view** (`big-button` makes it 52px on teach-mode views: 「開始上課」). `secondary-button`: white with a light grey border, ink text, a 1px lift on hover and a pressed state (primary lifts the same way), for a form's own submit when the view's filled action is elsewhere. `text-link`: the default for everything else (bold; `is-plain` for utilities like 修改, 取消, 更多); on touch screens it gets a 44px box. `manna-button`: gold outline, only for asking Manna. `ghost-button`, `small-button`, `decision-button` remain inside screens not rebuilt yet. Disabled is three channels: opacity or muted colour, `not-allowed`, the native attribute.
- **Ruled rows** (`.rows` > `.row`): the replacement for cards. A row is a title (sans 18px, a link), one quiet meta line (14px muted), and at the right a short fact with at most one or two text actions (`.side`). What a row opens in place (a note, the classes to assign) spans the row beneath it (`.row-more`). Ordered lists number their titles (`.num`). Under 760px the side drops under the title.
- **Focal block** (`.next`): on the home and class pages, the next lesson: a quiet kicker (「接下來 · 高一 3 班」), the title as the page's `h1`, one sentence, and the filled 「開始上課」 with 「繼續備課」 as a text link. Closed by a strong rule.
- **Page head** (`.page-head`, `.crumbs`, `.lead`, `.head-row`): crumbs in 14px muted, the `h1`, a lead paragraph; a `head-row` puts a section's one action at the right of its heading.
- **Forms opened in place** (`.inline-form-block`): between two strong rules, no frame. Not for add / upload / new actions: those use the Dialog below.
- **單元想法 on the unit page** (`.unit-ideas`, `.ledger`): one summary line under the lessons, 「單元想法：寫了 2/8 題」 with CoRe in small print, that opens on click to the eight questions as a ledger (written ones in ink with an accent Q number, empty ones muted with 「寫一點」) and a link to the 單元想法 page. It never gates anything. Units, lessons and the list of every unit (備課庫, `/units`, reached from 班級) all count as 班級 in the top bar.
- **Dialog** (`web/src/components/Dialog.tsx`, `.scrim` > `.dialog`): every add / upload / create-new action (上傳檔案, 貼上文字, 加入網址, 上傳我的學習單, 新增班級, 新增單元, 新增一節課例) opens this one modal instead of unrolling a form on the page; no second modal system. It is `role="dialog"` (or `alertdialog`), `aria-modal`, labelled by its heading. Focus starts on the first field, Tab stays inside, Escape, × and 取消 close it, and focus returns to the button that opened it. A form inside reports `useDialogGuard({ busy, dirty })`: while `busy` (saving, uploading) it cannot be closed; while `dirty` closing first asks 「要放棄剛才輸入的內容嗎？」 inside the dialog (繼續編輯 / 放棄並關閉); a click on the backdrop is the same as Escape. Errors show inside the dialog. On success the dialog closes and the page says what happened. Forms opened in place (`.inline-form-block`) remain only for onboarding invitations and edit-in-place fields, not for adding things.
- **Fields.** Label above (14px, 600), helper below in muted 14px, a 1px `--color-field-edge` border that never changes width. Selects keep native behaviour with a drawn chevron.
- **Manna content** (`help-entry`, `ai-suggestion`, `candidate-card`, `ai-status`, `manna-label`): a gold well, no border; it opens with the star and 「Manna」; its sources sit under a gold hairline; secondary text inside it is `--color-ink-2`. On the lesson page her panel sits in the side column, a step quieter than the plan.
- **Lesson workspace** (`.guided` > `.rail` + `.stage`, `sop.css`): the whole lesson page. The progress rail (232px, sticky; under 760px one line, 「第 3 步，共 7 步　看全部步驟」) lists the seven steps, each with what it already holds, and ends below a hairline with 「產生學習單」 (`.rail-end`); a tick means "has something", never "required". The page opens on the step of the last visit, else step 1. The stage shows one step: its title (30px) with 「對照教材」 at the right, a lead sentence, blocks parted by space, and at the bottom 上一步 / 先略過這一步 / the view's one filled button, 「下一步：…」; on step 7 that button is the big 「產生學習單」. 「開始上課」 is a secondary button in the page head on every step.
- **Teacher's writing** (`.mine`): ink on a ruled line with quiet tools (修改, 刪除) at the right. A form opened in place (`.stage-form`) sits between two strong rules.
- **Manna in a step** (`.manna-well`, `.decide`): a gold well directly under the paragraph or objective type it is about: star, 「Manna 建議 · 第 3 段」, the suggestion, 為什麼建議, sources under a gold hairline, then 採用 (ink-blue edge) / 修改後採用 / 不用. While she works the same well says what she is doing and for how long (`.manna-progress`). 「尚未完成課綱連結」 is the one amber tag (`.warn-tag`).
- **Print** (`.material-sheet`, `.sop-quote`, `.material-viewer`): the source text is serif on the lightest sheet with `¶n` in the margin; a paragraph quoted above its tasks uses the same face without the sheet. 「對照教材」 slides the sheet in from the left: beside the step from 1100px up (the rail gives way), over the page below that.
- **Collapsible sections** (`plan-section`): **retired** with 本節概要 (the lesson page's old first view). Its CSS is still in `app.css`.
- **Saved state** (`.saved`): plain words at the right of a working page's head: 「會自動儲存」, 「尚未儲存」, 「儲存中…」, 「已自動儲存 14:32」, or 「沒有存到：…　再試一次」. There is no save button on the lesson page.
- **Panels** (`panel-card`): **retired.** Screens not rebuilt yet still carry the class; it is now only a block parted from the one before it by space and one hairline. Do not use it in new code. A page has at most one layer of filled surface.
- **Tags** (`soft-tag`, `status-pill`, `record-pill`): 13px, only where they carry a fact (a mark's kind, a selection count). No 「選用」「可略過」 labels: what can be skipped is said once, in a sentence.
- **Notices:** a tinted well (`info`, `success`, `warn`), ink text, no side stripe.
- **Leave confirm** (`leave-confirm`): when a page with unsaved work is left, a dialog over the scrim asks 儲存後離開 / 留在這頁 (focused first, also Escape) / 不儲存，直接離開. Never `window.confirm`.
- **In-class console:** the lesson title with the clock (44px sans, tabular figures) at its right; two big marks (順利 green, 卡住 amber; 2px outline, an icon, the word and a short caption; 200px tall, a quarter of the screen each on a phone); then what was marked, newest first, as ruled rows with quiet corrections.

## Do and don't

Do:
- Put every new colour, size and face into `tokens.css` first, then use the token.
- Give each view exactly one `primary-button`; check it with the page's other actions demoted to `text-link`.
- Mark anything the AI produced with the Manna gold, and show its sources next to it.
- Give every control `:focus-visible`, `:hover` (behind `@media (hover: hover)`), and a disabled state.
- Check new screens from 320px to 1920px: no sideways scroll, controls 44px on touch.

Don't:
- Put a border around a section, or a card inside a card. Use space, a heading and hairlines.
- Ask for the class on a page. It is context (principle 6).
- Use system words on controls (版本, Phase, RAG, 選用), tracked capitals, or a label above a head that only names it.
- Use emoji as icons, gradients, glows, or shadows on anything that does not float.
- Use colour as the only signal, or grey text on a tinted surface.
- Load fonts or anything else from a third-party CDN.
- Animate `width`, `height`, `top`, `left` or `all`.

## Not done (yet)

- **Lesson workspace, later:** Manna inside steps 4 to 7 (they are manual editors now), automatic procedural scaffolds, the version history behind the saved line, annotations on the material, and the 「問資深教師」 button (step 4).
- **學習單 (step 3)** and **AI 資深教師 (step 4):** the destination exists as an honest placeholder; the senior-teacher button is not in the app yet.
- **Screens not rebuilt:** the 單元想法 (CoRe) editor, 課後補寫, 備課偏好, the first-use survey and admin still use their earlier layout (wells for steps and choices, `ghost-button` utilities, more than one filled button on the CoRe editor and admin).
- **Dark mode.** Left out on purpose: the gold-on-wash Manna role and the two record tones need their own pairs and a contrast pass in both modes.

## Exports

`web/src/styles/tokens.css` (tokens) · `base.css` (elements, focus, motion policy) · `components.css` (controls, fields, tags, notices) · `shell.css` (demo banner, top bar, frames, sign-in) · `app.css` (unit page, 單元想法, lesson planning, preferences, admin) · `record.css` (the console's parts, 課後補寫, the phone remote) · `views.css` (rows, the focal block, page heads, text links, the dialog, the console layout) · `sop.css` (the lesson workspace: rail, steps, Manna wells in a step, the print sheet and the material viewer; loaded last).

## Bright restyle (colour logic and form)

- **Colour logic.** The chrome has no colour: white page, `#f7f7f7` / `#ededed` surfaces, near-black ink, grey hairlines. One blue (`#1f5eff`) means "you can act here". Any other colour comes from content (worksheets, materials, images), never from chrome. Gold stays only as the AI marker, quieter than before. State colours (success, warn, error) stay functional and calm.
- **Form.** Headings are the sans family at 700 with tight tracking; the serif (`--font-print`) is kept only on worksheet and material sheets. Radii are soft (controls 10px, cards and dialogs 16px, pills for tags and chips). Flat: hairlines and space do the work; shadows only on floating things (popovers, dialogs, sticky bars) and lighter than before. The selected sidebar item is a light-grey pill with ink text.
- **Light only.** The page does not follow a dark colour scheme.
