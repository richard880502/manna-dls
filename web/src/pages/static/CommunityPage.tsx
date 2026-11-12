// Static port of the prototype's 社群動能地圖 (demo data only).
import { useEffect, useMemo, useState } from "react";
import { Phase2Tag } from "../../components/common";

type Point = [number, number];
interface Geometry {
  type: "Polygon" | "MultiPolygon";
  arcs: number[][] | number[][][];
  properties?: { COUNTYNAME?: string };
}
interface Topology {
  arcs: Point[][];
  transform: { scale: Point; translate: Point };
  objects: { layer1: { geometries: Geometry[] } };
}

const COUNTS: Record<string, number> = {
  臺北市: 68, 新北市: 46, 桃園市: 32, 新竹市: 24, 新竹縣: 19, 苗栗縣: 18, 臺中市: 55, 彰化縣: 30,
  南投縣: 17, 雲林縣: 16, 嘉義市: 10, 嘉義縣: 12, 臺南市: 38, 高雄市: 41, 屏東縣: 15, 宜蘭縣: 22,
  花蓮縣: 16, 臺東縣: 14, 澎湖縣: 8, 金門縣: 7, 連江縣: 4,
};
const PROFILES: Record<string, Array<[string, string, string]>> = {
  新竹市: [
    ["Mavis", "成德高中", "閱讀素養"],
    ["Effie", "新竹高中", "寫作教學"],
    ["Emily", "竹科實中", "AI 輔助教學"],
  ],
};
const AVATARS: Record<string, string> = {
  Mavis: "/assets/avatars/mavis-ai-thumb.jpg",
  Effie: "/assets/avatars/effie-ai-thumb.jpg",
  Emily: "/assets/avatars/emily-ai-thumb.jpg",
};
const ALIASES: Record<string, string> = {
  台北市: "臺北市", 台北縣: "新北市", 桃園縣: "桃園市", 台中市: "臺中市", 台中縣: "臺中市",
  台南市: "臺南市", 台南縣: "臺南市", 高雄縣: "高雄市", 台東縣: "臺東縣",
};
const MAINLAND = new Set([
  "基隆市", "臺北市", "新北市", "桃園市", "新竹市", "新竹縣", "苗栗縣", "臺中市", "彰化縣", "南投縣",
  "雲林縣", "嘉義市", "嘉義縣", "臺南市", "高雄市", "屏東縣", "宜蘭縣", "花蓮縣", "臺東縣",
]);
const ISLAND_MARKERS: Record<string, Point> = { 連江縣: [72, 86], 澎湖縣: [74, 380], 金門縣: [76, 494] };
const MARKER_OFFSETS: Record<string, Point> = {
  基隆市: [42, -14], 臺北市: [-20, -14], 新北市: [20, 30], 嘉義市: [-18, -10], 嘉義縣: [18, 12],
};
const WIDTH = 460;
const HEIGHT = 640;
const PADDING = 24;

const display = (name: string) => name.replace("臺", "台");

function arcPoints(topo: Topology, arcIndex: number): Point[] {
  const index = arcIndex < 0 ? ~arcIndex : arcIndex;
  let x = 0;
  let y = 0;
  const pts = (topo.arcs[index] ?? []).map(([dx, dy]): Point => {
    x += dx;
    y += dy;
    return [x * topo.transform.scale[0] + topo.transform.translate[0], y * topo.transform.scale[1] + topo.transform.translate[1]];
  });
  return arcIndex < 0 ? pts.reverse() : pts;
}

function rings(geometry: Geometry): number[][][] {
  return geometry.type === "Polygon" ? [geometry.arcs as number[][]] : (geometry.arcs as number[][][]);
}

function ringPoints(topo: Topology, ring: number[]): Point[] {
  return ring.flatMap((arc, i) => {
    const pts = arcPoints(topo, arc);
    return i ? pts.slice(1) : pts;
  });
}

interface CountyShape {
  name: string;
  paths: string[];
  marker: Point;
  count: number;
}

function buildShapes(topo: Topology): CountyShape[] {
  const byCounty = new Map<string, Geometry[]>();
  for (const g of topo.objects.layer1.geometries) {
    const raw = g.properties?.COUNTYNAME;
    if (!g.type || !raw) continue;
    const name = ALIASES[raw] ?? raw;
    byCounty.set(name, [...(byCounty.get(name) ?? []), g]);
  }
  const all: Point[] = [];
  byCounty.forEach((geoms, name) => {
    if (!MAINLAND.has(name)) return;
    for (const g of geoms) for (const poly of rings(g)) for (const ring of poly) all.push(...ringPoints(topo, ring));
  });
  const xs = all.map((p) => p[0]);
  const ys = all.map((p) => p[1]);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  const scale = Math.min((WIDTH - PADDING * 2) / (maxX - minX), (HEIGHT - PADDING * 2) / (maxY - minY));
  const project = ([x, y]: Point): Point => [PADDING + (x - minX) * scale, PADDING + (maxY - y) * scale];

  const shapes: CountyShape[] = [];
  byCounty.forEach((geoms, name) => {
    const isIsland = name in ISLAND_MARKERS;
    const paths = isIsland
      ? []
      : geoms.map((g) =>
          rings(g)
            .map((poly) =>
              poly
                .map((ring) => ringPoints(topo, ring).map((pt, i) => `${i ? "L" : "M"}${project(pt).join(" ")}`).join("") + "Z")
                .join(""),
            )
            .join(""),
        );
    const projected = geoms.flatMap((g) => rings(g).flatMap((poly) => poly.flatMap((ring) => ringPoints(topo, ring)))).map(project);
    const cx = projected.reduce((t, [x]) => t + x, 0) / Math.max(projected.length, 1);
    const cy = projected.reduce((t, [, y]) => t + y, 0) / Math.max(projected.length, 1);
    const [ox, oy] = MARKER_OFFSETS[name] ?? [0, 0];
    shapes.push({ name, paths, marker: ISLAND_MARKERS[name] ?? [cx + ox, cy + oy], count: COUNTS[name] ?? 3 });
  });
  return shapes;
}

export function CommunityPage() {
  const [topo, setTopo] = useState<Topology | null>(null);
  const [failed, setFailed] = useState(false);
  const [selected, setSelected] = useState("新竹市");

  useEffect(() => {
    let alive = true;
    fetch("/assets/tw-county-2010.topo.json")
      .then((r) => (r.ok ? (r.json() as Promise<Topology>) : Promise.reject(new Error("unavailable"))))
      .then((t) => alive && setTopo(t))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, []);

  const shapes = useMemo(() => {
    if (!topo) return [];
    try {
      return buildShapes(topo);
    } catch {
      return [];
    }
  }, [topo]);

  const profiles = PROFILES[selected] ?? [["社群教師", `${display(selected)} · 示範資料`, "期待交流"]];

  return (
    <section>
      <div className="page-intro">
        <p className="eyebrow warm">06 · Community pulse</p>
        <h2>社群動能地圖</h2>
        <p>點選縣市，看看願意公開參與的教師、最新交流與近期研習。圓點大小代表示範資料中的參與教師數。</p>
        <Phase2Tag />
      </div>
      <div className="community-layout">
        <section className="panel-card community-map-card" aria-labelledby="community-map-title">
          <div className="card-heading">
            <div>
              <p className="eyebrow warm">Community pulse</p>
              <h3 id="community-map-title">台灣教師社群</h3>
            </div>
            <span className="soft-tag">示範資料</span>
          </div>
          <div className="community-map-wrap">
            <div className="community-map" role="img" aria-label={`${display(selected)}目前有 ${COUNTS[selected] ?? 0} 位示範教師參與`}>
              {failed && <p className="empty-preview">地圖資料暫時無法載入。</p>}
              {shapes.length > 0 && (
                <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} aria-hidden="true">
                  {shapes.flatMap((s) =>
                    s.paths.map((d, i) => (
                      <path
                        key={`${s.name}-${i}`}
                        d={d}
                        className={`county-shape${s.name === selected ? " is-selected" : ""}`}
                        onClick={() => setSelected(s.name)}
                      />
                    )),
                  )}
                  {shapes.map((s) => (
                    <g
                      key={`m-${s.name}`}
                      className={`map-marker${s.name === selected ? " is-selected" : ""}`}
                      onClick={() => setSelected(s.name)}
                    >
                      <circle cx={s.marker[0]} cy={s.marker[1]} r={Math.max(8, Math.min(24, 4 + Math.sqrt(s.count) * 2.1))} />
                      <text x={s.marker[0]} y={s.marker[1]}>
                        {s.count}
                      </text>
                    </g>
                  ))}
                </svg>
              )}
            </div>
            <p className="community-map-legend">
              <span />
              圓點大小＝參與老師數
            </p>
          </div>
        </section>
        <aside className="community-side">
          <section className="panel-card community-panel">
            <div className="card-heading">
              <div>
                <p className="eyebrow warm">Public profiles</p>
                <h3>{display(selected)}教師</h3>
              </div>
            </div>
            <div className="community-teacher-list">
              {profiles.map(([name, school, topic]) => (
                <article key={name} className="community-teacher">
                  <span className="community-teacher-avatar">
                    {AVATARS[name] ? <img src={AVATARS[name]} alt={`${name} 的 AI 生成肖像`} /> : name.slice(0, 1)}
                  </span>
                  <div>
                    <strong>{name}</strong>
                    <p>
                      {school} · {topic}
                    </p>
                  </div>
                </article>
              ))}
            </div>
          </section>
          <section className="panel-card community-panel">
            <div className="card-heading">
              <div>
                <p className="eyebrow warm">Recent posts</p>
                <h3>最新貼文</h3>
              </div>
            </div>
            <div className="community-feed">
              <article>
                <span className="feed-dot">
                  <img src="/assets/avatars/ray-ai-thumb.jpg" alt="Ray 的 AI 生成肖像" />
                </span>
                <div>
                  <strong>如何設計有效的英文口說暖身活動？</strong>
                  <p>Ray｜嘉義高商 · 2 小時前</p>
                </div>
              </article>
              <article>
                <span className="feed-dot">
                  <img src="/assets/avatars/elma-ai-thumb.jpg" alt="Elma 的 AI 生成肖像" />
                </span>
                <div>
                  <strong>雙語課堂中如何兼顧語言與內容學習？</strong>
                  <p>Elma｜花蓮高中 · 5 小時前</p>
                </div>
              </article>
            </div>
          </section>
          <section className="panel-card community-panel">
            <div className="card-heading">
              <div>
                <p className="eyebrow warm">Upcoming</p>
                <h3>最近研習／共備</h3>
              </div>
            </div>
            <div className="community-events">
              <article>
                <span>▣</span>
                <div>
                  <strong>素養導向閱讀教學設計工作坊</strong>
                  <p>5/28（三）13:30–16:30</p>
                </div>
              </article>
              <article>
                <span>♧</span>
                <div>
                  <strong>高中英文寫作教學共備會</strong>
                  <p>5/30（五）09:00–12:00</p>
                </div>
              </article>
            </div>
          </section>
        </aside>
      </div>
    </section>
  );
}
