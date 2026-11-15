# Worksheet generator with the real model (gemma-4-31B-it, NCHC gateway)

2026-11-15. Why warm-up and main tasks came back empty with the production model, what fixed it,
and the measurements before and after. Model: `gemma-4-31B-it` through
`portal.genai.nchc.org.tw` (LiteLLM → vLLM 0.32, `response_format: json_schema`, strict).

## Root cause: the JSON schemas reached the model in alphabetical order

The gateway's constrained decoding makes the model write object properties **in the order the
schema lists them**. The server built its schemas with `serde_json::json!`, and without the
`preserve_order` feature `serde_json` stores objects as a `BTreeMap`, so every schema was sent
with its properties sorted alphabetically:

| schema | order the model was forced into |
|---|---|
| tasks block | `block_id, design_reason_zh, info_function, instructions_en, items, objective_refs, paragraph_nos, student_tasks, title_en` |
| tasks item | `answer_en, answer_format, evidence, item_id, options, prompt_en, tier` (the answer before the question) |
| warm-up activity | `answer_format, instructions_en, prompts_en, purpose_zh, student_tasks, title_en` |
| scaffold set | `instructional, procedural, target, verbal` |

The model follows its own idea of the object. When it has written what it thinks is the last
field (for the warm-up activity, `student_tasks`; for a tasks block, `items`) it wants to close
the object, but the grammar still requires `title_en` / `objective_refs`. The only tokens the
grammar allows that are not the next key are whitespace, so the model writes whitespace until
`max_tokens`. [`runaway-warmup-alphabetical.txt`](runaway-warmup-alphabetical.txt) is one captured
warm-up output: one complete activity, then 7,074 whitespace characters. All 18 runaways
captured (10 in pipeline runs: 6 warm-up, 2 tasks, 2 scaffolds; 8 in replays) had this shape: a
valid JSON prefix 500–1,300 characters long that stops right before an alphabetically-late
required key (warm-up after `student_tasks`, tasks after `items` or `student_tasks`, scaffolds
after `target`), then only spaces and newlines. None was a repeated value or an enum loop.

The research harness (`research/sop-validation`, Python) keeps dict order, which is why the
validation in `docs/LLM_VALIDATION.md` did not see this.

### Experiments on captured requests

Each request was rebuilt exactly as the server sent it (captured through a local proxy) and
replayed against the gateway; every try changed the temperature so the gateway cache was never
hit.

| request | change | ran to `max_tokens` | latency |
|---|---|---|---|
| warm-up | none (alphabetical) | 3 / 4 | 18 s each |
| warm-up | properties in written (`required`) order | **0 / 6** | 5–7 s |
| tasks | none (alphabetical) | 1 / 2 (plus 2 / 2 in the pipeline) | 15–36 s |
| tasks | written order | **0 / 3** | 15–16 s |
| warm-up, alphabetical | `repetition_penalty: 1.05` | 2 / 2 | 18 s |
| warm-up, alphabetical | `frequency_penalty: 0.3` | 2 / 2 (worse: 42,000 spaces) | 41–45 s |
| warm-up | `structured_outputs: {json, disable_any_whitespace: true}` | accepted by the gateway but whitespace was not disabled (output still indented; the alphabetical variant still ran away) | — |
| warm-up | `guided_whitespace_pattern: ""` | ignored | — |

Penalties and whitespace switches do not help; the property order does.

## Fixes

1. `serde_json` with `preserve_order`: schemas are sent in the order they are written. A unit
   test walks every LLM schema in the server (worksheet, lesson SOP, core assist, plan
   suggestions, material focuses) and fails if any object's `properties` order differs from its
   `required` order.
2. A runaway is kept for diagnosis: `ProviderError::MaxTokens { partial }`; the
   `ai_interactions` row stores `{truncated_at_max_tokens, runaway: whitespace|repetition|long,
   chars, longest_whitespace, runaway_excerpt}` (model output only, head and tail with long
   whitespace collapsed; scrubbed like `evidence` when a material is deleted).
3. Per-call budgets follow the request size (≈ 2× the longest measured output): tasks
   `1500 + 600 × paragraphs` (3000–6000; measured ≈ 500/paragraph + 500), warm-up 2000 (measured
   ≤ 810), scaffolds `800 + 350 × targets` (2000–5000; measured ≈ 220/target), exit ticket 1200
   (measured ≤ 434). A runaway now wastes at most ~12–30 s instead of 18–36 s.
4. Things the code can fix completely no longer cost a repair call; they pass with the repair
   recorded (`checks[].detail`, `repairs`):
   - spliced evidence → longest verbatim piece of ≥ 5 words, else cleared (as the SOP analysis
     drops spliced evidence);
   - scaffold targets matched however they are spelled (`Warm-up`, `warmup`, `b1`, `[B1]`); a
     warm-up set when the warm-up failed and was not sent is dropped (this was the
     `targets_match_blocks_and_warmup` failure on every first scaffolds call before);
   - Chinese glosses in a word bank (`behavior (行為)`) are removed;
   - a key word's context sentence that is not verbatim (gemma drops the quotation marks of
     dialogue) is replaced by the sentence the code found.
5. Blocks with nothing to print (an empty block left by a failed step) are left out of the
   student and teacher sheets, DOCX and PDF; letters close up. The editor still shows the block,
   its message, and a note that it is skipped when printing.

Not done, because the data did not call for it: splitting tasks into one call per paragraph
(one call for 3–6 paragraphs takes 11–20 s and passed 11/12 first time), streaming with an
early-stop detector (0 runaways in 52 calls after the fix; the smaller budgets bound the cost
if one happens), prompt changes.

## Before / after: full generations with the real model

Full path: lesson with SOP objectives → worksheet → generate → poll → export, through the HTTP
API. "Before" is the production image (`e7dce67`): 3 generations on a local build of the same
commit and 2 on a throwaway container of the production image. "After" is 12 generations on a
throwaway container built from this branch (9 on `9b1ba22`, 3 on `c21fba8`), 4 per passage,
each with a slightly different class profile so no request was a cached copy. Passages and
settings: [`passages.md`](passages.md) (262 / 142 / 326 words, 4 / 3 / 6 paragraphs).

| step | before: step succeeded | before: first call clean | before: calls run to max_tokens | before: step time (median / max) | after: step succeeded | after: first call clean | after: calls run to max_tokens | after: step time (median / max) |
|---|---|---|---|---|---|---|---|---|
| tasks | 4 / 5 | 4 / 5 | 2 / 6 | 15 s / 70 s | **12 / 12** | 11 / 12 | **0 / 13** | 15 s / 20 s |
| warm-up | **0 / 5** | 0 / 5 | **10 / 10** | 35 s / 37 s | **12 / 12** | 9 / 12 ¹ | **0 / 15** | 6 s / 9 s |
| scaffolds | 4 / 4 ² | 0 / 4 | 2 / 8 | 20 s / 39 s | **12 / 12** | 12 / 12 | 0 / 12 | 9 s / 12 s |
| exit ticket | 5 / 5 | 5 / 5 | 0 / 5 | 3 s / 4 s | 12 / 12 | 12 / 12 | 0 / 12 | 3 s / 4 s |
| whole generation | warm-up empty 5 / 5, tasks empty 1 / 5 | | | 74–112 s (median 88 s) | nothing empty 12 / 12 | | | 28–43 s (median 34 s) |

¹ The 3 misses were `context_sentence_verbatim`, all in the first 9; `c21fba8` repairs it in
code, and its 3 generations were 12 / 12 clean first calls.
² One generation had no blocks to scaffold (tasks failed), so the step was skipped.

The teacher's reported runs (same production image, the `habits` passage) match the "before"
column: warm-up 6 / 6 calls to `max_tokens`, tasks 3 / 4.

Whole-generation times are measured by polling every 3 s.

## Sample exports (last run, `c21fba8`)

| passage | student | teacher |
|---|---|---|
| The Power of Small Habits (basic) | [habits-student.pdf](habits-student.pdf) | [habits-teacher.pdf](habits-teacher.pdf) |
| The Night Market Stall (basic) | [market-student.pdf](market-student.pdf) | [market-teacher.pdf](market-teacher.pdf) |
| Cooling the Urban Heat Island (standard) | [heat-student.pdf](heat-student.pdf) | [heat-teacher.pdf](heat-teacher.pdf) |

## Method notes

- The API key never left the production host: requests were piped through `ssh manna` to a
  `curl` that read the key there. About 110 model calls in total (plus 26 gateway cache hits).
- The throwaway stack (`manna-wsfix` network, `wsfix-db`, `wsfix-app` on `127.0.0.1:18090`, the
  two images built for it) was removed afterwards; production containers were not touched.
