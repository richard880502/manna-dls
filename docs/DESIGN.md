# Design: Manna DLS

The visual system of the web app (`web/`). Every screen reads it; change it on purpose. The source of truth for values is `web/src/styles/tokens.css`; this file says what each token is for and how the parts are put together. The direction and the reasons are in [`DESIGN_OVERHAUL.md`](DESIGN_OVERHAUL.md); the approved mockups are in [`design/overhaul/`](design/overhaul/index.html), and before/after screenshots of what is built so far are in [`design/overhaul/implemented/`](design/overhaul/implemented/).

**Status:** overhaul steps 1 (shell and list pages) and 2 (the seven-step lesson workspace, direction A) are in the app; the CoRe editor, 課後補寫, preferences and admin keep their layout and take the new tokens, heads and frameless panels.

## System

- Genre: editorial, at app scope (screens someone works in, not pages someone reads once)
- Shape: a 56px top bar with three destinations; pages are a column of sections
- Vibe: "a calm research notebook in two inks", with a third voice for print
- Axes: light paper / serif heads / ink-blue accent. Posture: restrained, plus one gold with one job
- Modes: light only (see "Not done")

## Principles (from the overhaul)

1. **One view, one thing.** One filled button per view; everything else is an outline button or a text link.
2. **Space, type size and hairlines part sections, not boxes.** Only three things are filled surfaces: what Manna suggests (gold), source material and worksheets (the print sheet, from step 2).
3. **Three voices.** The teacher's own words and actions are ink blue. Whatever Manna (the AI) adds sits in gold and carries its sources. Source material is print: serif on the lightest sheet, paragraph numbers in the margin (step 2).
4. **Nothing blocks.** No required fields; every step can be skipped; what the AI produces is only written in when the teacher says so.
5. **The teacher's words.** No 「版本」「Phase」「RAG」「選用」 on controls; navigation and buttons are in Chinese. CoRe is called 「單元想法」, with CoRe in small print.
6. **The class is context, not a field.** The class is the one the teacher came from; it appears once, in the breadcrumb. It is asked for in exactly one place: starting a lesson from the library when several classes use its unit.

## Tokens

### Colour

| Token | Value | Job |
| --- | --- | --- |
| `--color-paper` | `#f7f8fa` | the page |
| `--color-paper-2` / `-3` | `#eef1f5` / `#e3e8ee` | neutral wells and quiet tags / hover, progress tracks |
| `--color-field`, `--color-sheet` | `#fdfdfe` | inputs; the print sheet (material, worksheets) |
| `--color-ink` | `#1c2533` | text and heads (14.5:1 on paper) |
| `--color-ink-2` | `#465163` | secondary text, lead paragraphs (7.6:1) |
| `--color-muted` | `#5a6677` | meta lines, hints, crumbs (5.5:1 on paper, 5.1:1 on paper-2) |
| `--color-field-edge` | `#7d8796` | input and outline-button edges (3.4:1 or better) |
| `--color-rule` / `-strong` | `#dde2e9` / `#b9c1cc` | hairlines between rows / the rule under a focal block or around an open form |
| `--color-accent` (+ `-hover`, `-press`, `-ink`, `-text`) | `#2a5580` | the teacher's ink: the filled action, the current destination, links |
| `--color-accent-wash` / `-wash-2` / `-rule` | `#e6eef6` / `#d5e2f0` / `#8fb0cf` | selected tiles / pressed / link underline |
| `--color-focus` | `#1f5fd0` | the 3px focus ring (5.5:1 on paper) |
| `--color-star` | `#e2b33a` | Manna's star |
| `--color-manna-wash` / `-wash-2` / `-rule` / `-ink` | `#fbf3dc` / `#f4e6b8` / `#d9bd6a` / `#73500f` | everything Manna produced, and the button that asks her (ink on wash 6.6:1) |
| `--color-success`, `--color-warn-ink`, `--color-error` (+ washes) | | 順利 and saved / 卡住 and the demo banner / errors (each 5.5:1 or better on its wash) |

Rules: no colour literal outside `tokens.css`; never pure black or white as a surface; grey text never sits on a coloured surface; red and green never carry meaning alone (every state also has a word or an icon).

### Type

Self-hosted with `@fontsource-variable` (pinned, `font-display: swap`, sliced by Unicode range). No request leaves for a font CDN.

| Role | Face | Size / weight |
| --- | --- | --- |
| Heads, row titles, Q numbers, the lesson clock | Noto Serif TC, `--font-display` | 600 |
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

- **Top bar** (`.top`, 56px, sticky under the demo banner): the star and 「Manna」, the three destinations (班級, 備課庫, 學習單), and at the right the account links (知識庫管理 for the project lead and knowledge-base managers, 後台管理 for the project lead, 「研究編號 · 備課偏好」, 登出). Under 760px the three destinations move to a fixed bar at the bottom (60px) and the research ID drops out of the top bar.
- A page that takes over the bar renders into its slot (`TopBarSlot`): during a lesson the bar shows only 「上課中 · 已同步」 and 「結束本節課」, and the bottom bar is gone, so nothing invites a stray tap.
- **Frames** (`.workspace`): list pages 1040px; working pages with two columns (lesson, 單元想法, 課後補寫, admin) 1200px; the in-class console 880px; single-column text pages 800px.
- **Breakpoints:** 760px (shell, rows, single column), 1100px (the lesson page's side column goes under the main one), plus the older 75rem / 60rem / 40rem inside screens not rebuilt yet.

## Components

- **Buttons.** `primary-button`: filled accent, **one per view** (`big-button` makes it 52px on teach-mode views: 「開始上課」). `secondary-button`: neutral outline on the field colour, for a form's own submit when the view's filled action is elsewhere. `text-link`: the default for everything else (bold; `is-plain` for utilities like 修改, 取消, 更多); on touch screens it gets a 44px box. `manna-button`: gold outline, only for asking Manna. `ghost-button`, `small-button`, `decision-button` remain inside screens not rebuilt yet. Disabled is three channels: opacity or muted colour, `not-allowed`, the native attribute.
- **Ruled rows** (`.rows` > `.row`): the replacement for cards. A row is a title (serif 18px, a link), one quiet meta line (14px muted), and at the right a short fact with at most one or two text actions (`.side`). What a row opens in place (a note, the classes to assign) spans the row beneath it (`.row-more`). Ordered lists number their titles (`.num`). Under 760px the side drops under the title.
- **Focal block** (`.next`): on the home and class pages, the next lesson: a quiet kicker (「接下來 · 高一 3 班」), the title as the page's `h1`, one sentence, and the filled 「開始上課」 with 「繼續備課」 as a text link. Closed by a strong rule.
- **Page head** (`.page-head`, `.crumbs`, `.lead`, `.head-row`): crumbs in 14px muted, the `h1`, a lead paragraph; a `head-row` puts a section's one action at the right of its heading.
- **Forms opened in place** (`.inline-form-block`): between two strong rules, no frame.
- **Companion** (`.with-side` > `.companion`, `.ledger`): on the unit page, 單元想法 sits beside the lessons as a 320px column: a quiet progress line with CoRe in small print, the first five questions as a ledger (written ones in ink with an accent Q number, empty ones muted with 「寫一點」), and a link to all eight. It never gates anything.
- **Dialog** (`.scrim` > `.dialog`, `.radio-rows`): only for 「這節課在哪一班上？」. Focus starts on the chosen class, Tab stays inside, Escape cancels, focus returns to the button that opened it.
- **Fields.** Label above (14px, 600), helper below in muted 14px, a 1px `--color-field-edge` border that never changes width. Selects keep native behaviour with a drawn chevron.
- **Manna content** (`help-entry`, `ai-suggestion`, `candidate-card`, `ai-status`, `manna-label`): a gold well, no border; it opens with the star and 「Manna」; its sources sit under a gold hairline; secondary text inside it is `--color-ink-2`. On the lesson page her panel sits in the side column, a step quieter than the plan.
- **Lesson workspace** (`.guided` > `.rail` + `.stage`, `sop.css`): the progress rail (232px, sticky; under 760px one line, 「第 3 步，共 7 步　看全部步驟」) lists 本節概要 and the seven steps, each with what it already holds; a tick means "has something", never "required". The stage shows one step: its title (30px) with 「對照教材」 at the right, a lead sentence, blocks parted by space, and at the bottom 上一步 / 先略過這一步 / the view's one filled button, 「下一步：…」. 「開始上課」 is an outline button in the page head on every step, and the filled big button on 本節概要.
- **Teacher's writing** (`.mine`): ink on a ruled line with quiet tools (修改, 刪除) at the right. A form opened in place (`.stage-form`) sits between two strong rules.
- **Manna in a step** (`.manna-well`, `.decide`): a gold well directly under the paragraph or objective type it is about: star, 「Manna 建議 · 第 3 段」, the suggestion, 為什麼建議, sources under a gold hairline, then 採用 (ink-blue edge) / 修改後採用 / 不用. While she works the same well says what she is doing and for how long (`.manna-progress`). 「尚未完成課綱連結」 is the one amber tag (`.warn-tag`).
- **Print** (`.material-sheet`, `.sop-quote`, `.material-viewer`): the source text is serif on the lightest sheet with `¶n` in the margin; a paragraph quoted above its tasks uses the same face without the sheet. 「對照教材」 slides the sheet in from the left: beside the step from 1100px up (the rail gives way), over the page below that.
- **Collapsible sections** (`plan-section` inside `.plan-sections`): on 本節概要 everything past the first view is a hairline row with a count (「教學步驟（3）」). The body stays mounted while closed.
- **Saved state** (`.saved`): plain words at the right of a working page's head: 「會自動儲存」, 「尚未儲存」, 「儲存中…」, 「已自動儲存 14:32」, or 「沒有存到：…　再試一次」. There is no save button on the lesson page.
- **Panels** (`panel-card`): **retired.** Screens not rebuilt yet still carry the class; it is now only a block parted from the one before it by space and one hairline. Do not use it in new code. A page has at most one layer of filled surface.
- **Tags** (`soft-tag`, `status-pill`, `record-pill`): 13px, only where they carry a fact (a mark's kind, a selection count). No 「選用」「可略過」 labels: what can be skipped is said once, in a sentence.
- **Notices:** a tinted well (`info`, `success`, `warn`), ink text, no side stripe.
- **Leave confirm** (`leave-confirm`): when a page with unsaved work is left, a dialog over the scrim asks 儲存後離開 / 留在這頁 (focused first, also Escape) / 不儲存，直接離開. Never `window.confirm`.
- **In-class console:** the lesson title with the clock (44px serif, tabular figures) at its right; two big marks (順利 green, 卡住 amber; 2px outline, an icon, the word and a short caption; 200px tall, a quarter of the screen each on a phone); then what was marked, newest first, as ruled rows with quiet corrections.

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
