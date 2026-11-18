import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router";
import { api } from "../../api/client";
import "./mywork.css";

/** What a card needs to draw itself (the page turns a session or a worksheet into this). */
export interface WorkCardData {
  title: string;
  href: string;
  /** The type tag (「學習單」「課堂活動」…). */
  tag: string | null;
  /** The date, then whatever tells this work from another with the same title. */
  meta: string;
  /** The worksheet whose first page is the preview; null: a text preview. */
  worksheet: { id: number; version_no: number } | null;
  /** The text preview: a plan's first lines, a question and its answer, the request. */
  text: string | null;
}

/** A card this far below the screen already asks for its picture. */
const NEAR_SCREEN = "400px";

/** The preview picture's size (docs/API.md): the box shows its top, the box never changes size. */
const THUMB_WIDTH = 480;
const THUMB_HEIGHT = 679;

/**
 * A worksheet's first page as an object URL, asked for once the box comes near the screen (at
 * once where IntersectionObserver is missing) and once per version: a 409 (still being made) or
 * a failure is not retried, the card shows its text instead.
 */
function useThumbnail(worksheet: WorkCardData["worksheet"]) {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(() => typeof IntersectionObserver === "undefined");
  /** The outcome for one worksheet version: its picture, or `null` when there is none to show. */
  const [outcome, setOutcome] = useState<{ key: string; url: string | null } | null>(null);
  const id = worksheet?.id ?? null;
  const version = worksheet?.version_no ?? null;
  const key = id === null ? null : `${id}:${version}`;

  useEffect(() => {
    const el = ref.current;
    if (id === null || near || !el) return undefined;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) setNear(true);
      },
      { rootMargin: NEAR_SCREEN },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [id, near]);

  useEffect(() => {
    if (id === null || key === null || !near) return undefined;
    let alive = true;
    let made: string | null = null;
    api
      .worksheetThumbnail(id)
      .then((blob) => {
        if (!alive) return;
        made = URL.createObjectURL(blob);
        setOutcome({ key, url: made });
      })
      .catch(() => {
        if (alive) setOutcome({ key, url: null });
      });
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [id, key, near]);

  const current = outcome !== null && outcome.key === key ? outcome : null;
  return {
    ref,
    url: current?.url ?? null,
    failed: current !== null && current.url === null,
    fail: () => {
      if (key !== null) setOutcome({ key, url: null });
    },
  };
}

/**
 * The window onto the work: the top of its first page, or its first lines when there is no
 * page (or the page could not be had). Decoration: the title says what the card is.
 */
function Preview({ data }: { data: WorkCardData }) {
  const { ref, url, failed, fail } = useThumbnail(data.worksheet);
  const picture = data.worksheet !== null && !failed;
  let art: ReactNode;
  if (picture && url) {
    art = (
      <img
        src={url}
        alt={`${data.title} 學生版第一頁`}
        width={THUMB_WIDTH}
        height={THUMB_HEIGHT}
        loading="lazy"
        decoding="async"
        onError={fail}
      />
    );
  } else if (picture) {
    art = <span className="work-thumb-wait" aria-hidden="true" />;
  } else {
    art = (
      <p className="work-excerpt" aria-hidden="true">
        {data.text || data.title}
      </p>
    );
  }
  return (
    <div className={`work-thumb${picture ? " is-page" : " is-text"}`} ref={ref}>
      {art}
    </div>
  );
}

/**
 * One piece of work: the whole card is one link to it. The link's name is the title alone (the
 * picture and the text preview are decoration; the type and the meta are its description). A
 * button of its own, such as 刪除, goes in `tools`: outside the link, always visible.
 */
export function WorkCard({ data, tools }: { data: WorkCardData; tools?: ReactNode }) {
  const ids = useId();
  return (
    <li className="work-card">
      <Link
        className="work-card-link"
        to={data.href}
        aria-labelledby={`${ids}-title`}
        aria-describedby={`${ids}-tag ${ids}-meta`}
      >
        <Preview data={data} />
        <div className="work-card-body">
          <h3 className="work-card-title" id={`${ids}-title`} title={data.title}>
            {data.title}
          </h3>
          <span className="work-card-tag" id={`${ids}-tag`}>
            {data.tag && <span className="mywork-tag">{data.tag}</span>}
          </span>
          <span className="work-card-meta" id={`${ids}-meta`}>
            {data.meta}
          </span>
        </div>
      </Link>
      {tools && <div className="work-card-tools">{tools}</div>}
    </li>
  );
}
