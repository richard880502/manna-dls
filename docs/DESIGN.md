# Design: Manna DLS

The locked visual system for the web app (`web/`). Every screen reads it; change it on purpose. The source of truth for values is `web/src/styles/tokens.css`; this file says what each token is for and how the parts are put together. It follows the Hallmark design guidance ([Nutlope/hallmark](https://github.com/Nutlope/hallmark), MIT), adapted for an app used in Traditional Chinese.

## System

- Genre: editorial, at app scope (screens someone works in, not pages someone reads once)
- Shape: app screen, inside the existing shell (sidebar with nine destinations, a running head, panels)
- Theme: custom. Vibe: "a calm research notebook in two inks"
- Axes: light paper / roman serif heads / cool ink-blue accent. Posture: restrained, plus one seal colour with one job
- Modes: light only (see "Not done" at the end)

## The idea: two inks on one page

Teachers write in blue-black ink (藍黑墨水): their own words, their actions, what they select. Whatever **Manna** (the AI) adds sits in **gold**, the colour of the project's star mascot, and always carries its sources: help entries, suggestion cards, material candidates, the source count, the 需要協助 button. A teacher can tell at a glance which words are theirs and which came from Manna. That matters for a research platform that records whether an answer was written by the teacher, after AI help, or adopted from an AI example.

The signature object is the **CoRe ledger**: each learning focus is a ruled sheet of eight questions, Q1 to Q8 numbered in the left margin in the serif, the row being helped turning gold. The lesson page shows the linked focus's answers as the same ledger, set compact.

## Tokens

### Colour (OKLCH)

| Token | Value | Job |
| --- | --- | --- |
| `--color-paper` | `oklch(97.8% 0.005 245)` | page and panel ground |
| `--color-paper-2` | `oklch(95.4% 0.008 245)` | sidebar, neutral wells (題意說明, the teacher's own CoRe answers, step rows) |
| `--color-paper-3` | `oklch(92.6% 0.011 245)` | hover on paper-2, progress tracks |
| `--color-field` | `oklch(99.2% 0.002 245)` | inputs: the writing surface is the lightest thing on screen |
| `--color-ink` | `oklch(25% 0.035 252)` | text and heads (15:1 on paper) |
| `--color-ink-2` | `oklch(40% 0.03 250)` | secondary text, lead paragraphs |
| `--color-muted` | `oklch(50% 0.022 250)` | hints, meta lines, placeholders (5.6:1 on paper) |
| `--color-field-edge` | `oklch(61% 0.02 248)` | input borders (3:1 or better on every ground) |
| `--color-rule` / `-strong` | `oklch(88.5% …)` / `oklch(80% …)` | hairlines / outlines of quiet controls |
| `--color-accent` | `oklch(45% 0.1 245)` | the teacher's ink: the filled action, the current nav item, selection. Derived from the project's `#284b63` |
| `--color-accent-text` / `-wash` / `-rule` | | links and accent labels / selected tiles / secondary button outline |
| `--color-focus` | `oklch(50% 0.17 255)` | the focus ring, 5:1 on paper |
| `--color-star` | `oklch(80% 0.13 86)` | the star mark (gold on ink) and the selected county on the map |
| `--color-manna-wash` / `-wash-2` / `-rule` / `-ink` | | everything Manna produced: wells, chips, rules, labels (6.9:1) |
| `--color-success`, `--color-warn-*`, `--color-error` (+ washes) | | saved / 順利, draft / demo banner / 卡住, errors |

Rules: no colour literal outside `tokens.css`; never pure black or white as a surface; grey text never sits on a coloured surface; red and green never carry meaning alone (every state also has a word or an icon).

### Type

Self-hosted with `@fontsource-variable` (pinned, `font-display: swap`, sliced by Unicode range so a page only downloads the characters it shows). No request leaves for a font CDN.

| Role | Face | Size / weight |
| --- | --- | --- |
| Heads (`h1` to `h4`), unit and suggestion titles, Q numbers, step numbers | Noto Serif TC (思源宋體), `--font-display` | 600 |
| Everything else | Noto Sans TC (思源黑體), `--font-body` | 400, 600 for labels and buttons |
| Machine identifiers only: knowledge-base item codes, invite codes, SHA, prompt templates | system mono, `--font-mono` | 400 |

Scale (fixed, ratio about 1.18, no fluid heads): `--text-sm` 14 · `--text-md` 16 · `--text-lg` 18 · `--text-xl` 21 · `--text-2xl` 25 · `--text-3xl` 30 (`--text-xs` 13 is for mono codes only).

CJK rules: body never under 16px; prose `line-height` 1.75 (`--leading-body`), UI 1.55, heads 1.3 (CJK glyphs fill the em box, so heads never go under 1.25); measure `--measure: 40em` (about 40 characters); no tracking on CJK, no uppercase anywhere; `line-break: strict` and `text-autospace` so Chinese and English sit apart cleanly; `text-wrap: balance` on heads and titles.

### Space, shape, depth, motion

- **Space:** `--space-3xs` 2 · `2xs` 4 · `xs` 8 · `sm` 12 · `md` 16 · `lg` 24 · `xl` 32 · `2xl` 48 · `3xl` 64 · `4xl` 96. Panels pad `--card-pad` (24, 16 on a phone). Gutters `--gutter` 40 / 24 / 16.
- **Radius:** `--radius-control` 8 (buttons, inputs, wells, tiles) · `--radius-card` 12 (panels) · `--radius-pill` (tags, status, steppers) · `--radius-sm` 6 (code chips).
- **Depth:** borders, not shadows. A panel is paper plus a 1px `--color-rule` border. The only shadow is `--shadow-overlay`, on the 需要協助 sheet that opens over the bottom of a narrow screen.
- **Motion:** state only. `--dur-micro` 120ms for colour, `--dur-short` 220ms for a disclosure chevron or a switch; `--ease-out`, `--ease-in-out`; only `transform`, `opacity` and colours move. Under `prefers-reduced-motion: reduce` everything ends at once.
- **Layers:** `--z-sticky` (sidebar), `--z-sticky-nav` (demo banner), `--z-overlay` (help sheet).

## Layout

- Sidebar 16rem, sticky below the demo banner; under 60rem it becomes a top bar with a 選單 button.
- Workspace content is capped at `--container` (75rem) and centred; at 1920 the gutters grow, nothing else does.
- A running head on every screen: the stage (`01 · Study & Plan`) over the view title (`h1`). The page's own title is the larger `h2` below it.
- Breakpoints: 60rem (shell) and 40rem (type, single column), plus 75rem where two working panels sit side by side (CoRe with 需要協助, the lesson page, admin, 課後補寫). The unit page stays one column so no short panel leaves an empty half beside a long one.

## Components

- **Buttons.** `primary-button`: filled accent, one per region. `secondary-button`: accent outline on field. `ghost-button`: neutral outline for utilities (remove, cancel, close, move). `small-button`: 32px, never wraps. `decision-button`: the choices recorded on an AI result (回答／略過, 採用／修訂／拒絕, ratings), `aria-pressed` shows the pick. 40px tall, 44px on touch screens. Disabled is three channels: opacity 0.55, `not-allowed`, the native attribute.
- **Fields.** Label above (14px, 600), helper below in muted 14px, 1px `--color-field-edge` border that never changes width, the focus ring an inset outline. Selects keep native behaviour with a drawn chevron.
- **Choice tiles** (`pref-option`, `route-option`, `siop-choice`): the native radio or checkbox stays visible; checked tiles take the accent wash and border; the ring sits on the tile.
- **Panels** (`panel-card`): paper, 1px rule, radius 12. Inside a panel, parts are parted by hairlines or set in a well; never a bordered box inside a well, never a card inside a card.
- **Manna content** (`help-entry`, `ai-suggestion`, `candidate-card`, `ai-status`): a gold well, no border; its sources sit under a gold hairline, item codes in mono chips; secondary text inside it is `--color-ink-2`, never the muted grey. A rejected suggestion turns neutral grey rather than fading.
- **Ledger rows** (`core-cell`): hairline-ruled, Q number in the margin (inline on a phone), the row being helped in the gold wash, rows the prep route recommends first get an accent Q number and a 建議先寫 tag.
- **Tags** (`soft-tag`, `status-pill`, `core-status-*`, `route-tag`, `source-count`): body face 14px, sentence case, pill, tinted by meaning.
- **Notices:** a tinted well (`info` paper-2, `success`, `warn`), ink text, no side stripe.
- **Meta line** (`.eyebrow`): 14px muted body face above a head, only where it carries a fact (stage, version, route, focus).
- **Teach & Record:** two big bookmark buttons (順利 green, 卡住 amber, each with its SVG icon and word), sync state as a status chip with a live dot, each logged bookmark one row with its corrections at the right end (stacked on a phone), the phone remote with buttons a third of the screen tall.

## Do and don't

Do:
- Put every new colour, size and face into `tokens.css` first, then use the token.
- Mark anything the AI produced with the Manna gold, and show its sources next to it.
- Keep one filled button per region; give every control `:focus-visible`, `:hover` (behind `@media (hover: hover)`), `:active` and `:disabled`.
- Check new screens at 375px: no sideways scroll, controls 44px on touch.

Don't:
- Use tracked capitals, uppercase labels, or a label above a head that only names it.
- Use emoji as icons (the rating buttons use SVG), gradients, glows, or shadows on panels.
- Use colour as the only signal, or grey text on a tinted surface.
- Load fonts or anything else from a third-party CDN.
- Animate `width`, `height`, `top`, `left` or `all`.

## Not done (yet)

- **Dark mode.** Left out on purpose: the gold-on-wash Manna role, the map and the two record tones need their own re-derived pairs and a contrast pass in both modes; shipping it half-checked would be worse than not shipping it.
- **Copy kept as found:** meta lines joined with `·`, `→` after three links (a test reads one), and the English labels on the Phase 2 static pages. These are copy decisions, not styling.

## Exports

`web/src/styles/tokens.css` (tokens) · `base.css` (elements, focus, motion policy) · `components.css` · `shell.css` · `app.css` (screens) · `phase2.css` (static Phase 2 pages) · `record.css` (Teach & Record).
