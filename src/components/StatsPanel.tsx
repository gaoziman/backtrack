import { useMemo } from "react";
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { useStore } from "../store";
import type { StatsDto } from "../types";
import { IconBookmark, IconChart, IconX } from "./icons";

/// Token 估算系数：中英混合会话经验折中（字符数 ÷ 3.5）。
/// 仅作「约」值展示，永不换算成费用，避免假精确（见统计面板设计审计 D4）。
const TOKEN_DIVISOR = 3.5;

/// 热力图渲染周数（近 N 周，按天）。
const HEAT_WEEKS = 26;
/// 热力图单格边长 + 间距（px），固定尺寸保证紧凑（GitHub 贡献图式，不随容器膨胀）。
const HEAT_CELL = 13;
const HEAT_GAP = 3;

/// 大数字千分位/紧凑展示：≥1万用 K，≥100万用 M。
function compact(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1).replace(/\.0$/, "") + "M";
  if (n >= 10_000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "K";
  return n.toLocaleString("en-US");
}

/// 月份标签：2026-06 → 6月。
function monthLabel(ym: string): string {
  const m = ym.slice(5, 7).replace(/^0/, "");
  return `${m}月`;
}

/// 取某 CSS 变量当前值（Recharts 的 SVG fill/stroke 不接受 var()，必须传计算后的具体颜色）。
/// 兜底值 = 亮色 accent，仅在 getComputedStyle 异常时触发（理论上不会）。
function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#0d8e9c";
}

/// 近 N 天的会话数（趋势小标用）：从 by_day 末尾累加最近 N 天。
function recentCount(stats: StatsDto, days: number): number {
  if (stats.by_day.length === 0) return 0;
  const last = stats.by_day[stats.by_day.length - 1].day;
  const anchor = new Date(last + "T00:00:00Z");
  const lo = new Date(anchor);
  lo.setUTCDate(lo.getUTCDate() - (days - 1));
  const loKey = lo.toISOString().slice(0, 10);
  return stats.by_day
    .filter((d) => d.day >= loKey)
    .reduce((a, d) => a + d.count, 0);
}

export function StatsPanel() {
  const { stats, statsLoading, closeStats } = useStore();

  // 热力图数据：把 by_day 映射成「近 N 周 × 7 天」网格（列优先，最右为本周）+ 月份列标签。
  const heat = useMemo(() => buildHeat(stats), [stats]);

  if (statsLoading && !stats) {
    return (
      <div className="stats-view">
        <StatsHeader onClose={closeStats} />
        <div className="stats-empty">正在统计…</div>
      </div>
    );
  }

  if (!stats || stats.total_sessions === 0) {
    return (
      <div className="stats-view">
        <StatsHeader onClose={closeStats} />
        <div className="stats-empty">
          <IconChart size={28} />
          <p>暂无统计数据</p>
          <span>索引完成后这里会展示你的会话使用全貌</span>
        </div>
      </div>
    );
  }

  const tokenEst = Math.round(stats.total_body_chars / TOKEN_DIVISOR);
  const avgMsg = stats.total_sessions ? Math.round(stats.total_messages / stats.total_sessions) : 0;
  const recent30 = recentCount(stats, 30);
  const accent = cssVar("--accent");

  // 月趋势：补齐标签，喂 Recharts 面积图。
  const monthData = stats.by_month.map((m) => ({ label: monthLabel(m.month), count: m.count }));
  // 工具占比。
  const toolTotal = stats.by_tool.reduce((a, t) => a + t.count, 0) || 1;
  // 工具占比环形图：按 by_tool 顺序累积弧（dasharray 百分比，r=15.9 周长≈100）。
  let donutAcc = 0;
  // 目录排行 Top 6 + 占比。
  const topDirs = stats.top_dirs.slice(0, 6);
  const dirMax = topDirs[0]?.count || 1;

  return (
    <div className="stats-view">
      <StatsHeader onClose={closeStats} />
      <div className="stats-body">
        {/* KPI：英雄卡（总会话）+ 3 次级，统一白卡细边 */}
        <div className="kpi-grid">
          <div className="kpi hero">
            <div className="kpi-label"><IconBookmark size={13} />总会话</div>
            <div className="kpi-val num">{stats.total_sessions.toLocaleString("en-US")}</div>
            <div className="kpi-foot">
              涉及 {stats.distinct_dirs} 个目录
              {recent30 > 0 && (
                <span className="kpi-trend">
                  <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M7 17 17 7M9 7h8v8" /></svg>
                  近30天 +{compact(recent30)}
                </span>
              )}
            </div>
          </div>
          <div className="kpi">
            <div className="kpi-label">总消息</div>
            <div className="kpi-val num">{compact(stats.total_messages)}</div>
            <div className="kpi-foot num">平均 {avgMsg} 条 / 会话</div>
          </div>
          <div className="kpi">
            <div className="kpi-label">Token 估算</div>
            <div className="kpi-val num">~{compact(tokenEst)}</div>
            <div className="kpi-foot">约值 · 按正文字符估</div>
          </div>
          <div className="kpi">
            <div className="kpi-label">Fork 会话</div>
            <div className="kpi-val num">{stats.fork_count}</div>
            <div className="kpi-foot">谱系分叉数</div>
          </div>
        </div>

        {/* 月趋势(Recharts 面积图) + 工具占比(环 + 条) */}
        <div className="stats-row split">
          <div className="stats-card">
            <div className="stats-card-title">按月分布<span className="hint">会话数</span></div>
            <div className="chart-box">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={monthData} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
                  <defs>
                    <linearGradient id="areaFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor={accent} stopOpacity={0.28} />
                      <stop offset="100%" stopColor={accent} stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <XAxis dataKey="label" tick={{ fontSize: 11, fill: cssVar("--faint") }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fontSize: 11, fill: cssVar("--faint") }} axisLine={false} tickLine={false} width={34} />
                  <Tooltip
                    cursor={{ stroke: accent, strokeOpacity: 0.25 }}
                    contentStyle={{
                      background: cssVar("--panel"), border: `1px solid ${cssVar("--border")}`,
                      borderRadius: 8, fontSize: 12, color: cssVar("--text-hi"),
                    }}
                    labelStyle={{ color: cssVar("--text-lo") }}
                  />
                  <Area
                    type="monotone" dataKey="count" name="会话" stroke={accent} strokeWidth={2}
                    fill="url(#areaFill)" dot={{ r: 2.5, fill: accent, strokeWidth: 0 }}
                    activeDot={{ r: 4, fill: accent, strokeWidth: 0 }}
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="stats-card">
            <div className="stats-card-title">工具占比</div>
            <div className="tool-wrap">
              <svg className="donut" width="78" height="78" viewBox="0 0 42 42" aria-hidden="true">
                <circle cx="21" cy="21" r="15.9" fill="none" stroke="var(--panel-3)" strokeWidth="6" />
                {stats.by_tool.map((t) => {
                  const pct = (t.count / toolTotal) * 100;
                  const seg = (
                    <circle
                      key={t.tool} cx="21" cy="21" r="15.9" fill="none"
                      stroke={`var(--${t.tool})`} strokeWidth="6"
                      strokeDasharray={`${pct} ${100 - pct}`}
                      strokeDashoffset={25 - donutAcc}
                      strokeLinecap="round"
                    />
                  );
                  donutAcc += pct;
                  return seg;
                })}
              </svg>
              <div className="tool-bars">
                {stats.by_tool.map((t) => {
                  const pct = ((t.count / toolTotal) * 100).toFixed(1);
                  return (
                    <div className="tool-bar" key={t.tool}>
                      <div className="tool-bar-head">
                        <span className="nm"><span className={`d ${t.tool}`} />{t.tool === "claude" ? "Claude" : "Codex"}</span>
                        <span className="pct num">{t.count} · {pct}%</span>
                      </div>
                      <div className="tool-bar-track">
                        <div className="tool-bar-fill" style={{ width: `${pct}%`, background: `var(--${t.tool})` }} />
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
            <div className="tool-extra">
              <div className="ov-row"><span className="k">最早会话</span><span className="v num">{(stats.earliest || "").slice(0, 10) || "—"}</span></div>
              <div className="ov-row"><span className="k">正文总量</span><span className="v num">~{compact(stats.total_body_chars)} 字符</span></div>
            </div>
          </div>
        </div>

        {/* 活跃热力图(GitHub 式紧凑：固定 13px 小格 + 星期/月份轴) */}
        <div className="stats-row">
          <div className="stats-card">
            <div className="stats-card-title">活跃热力图<span className="hint">近 {HEAT_WEEKS} 周 · 按天</span></div>
            <div className="heat-area">
              <div className="heat-wd">
                <span /><span>一</span><span /><span>三</span><span /><span>五</span><span />
              </div>
              <div className="heat-main">
                <div className="heat-months">
                  {heat.months.map((m) => (
                    <span key={m.col} style={{ left: m.col * (HEAT_CELL + HEAT_GAP) }}>{m.label}</span>
                  ))}
                </div>
                <div
                  className="heat-grid"
                  style={{
                    gridTemplateRows: `repeat(7, ${HEAT_CELL}px)`,
                    gridAutoColumns: `${HEAT_CELL}px`,
                    gap: HEAT_GAP,
                  }}
                >
                  {heat.cells.map((lv, i) => (
                    <div key={i} className="heat-cell" style={{ background: heatColor(lv) }} title={heat.titles[i]} />
                  ))}
                </div>
              </div>
            </div>
            <div className="heat-legend">
              少
              {[0, 1, 2, 3, 4].map((lv) => <span key={lv} className="heat-cell" style={{ background: heatColor(lv) }} />)}
              多
            </div>
          </div>
        </div>

        {/* 最活跃目录(排名榜单：序号 + 名 + 工具色点 + 定宽迷你条 + 数值 + 占比) */}
        <div className="stats-row">
          <div className="stats-card">
            <div className="stats-card-title">最活跃目录<span className="hint">Top {topDirs.length}</span></div>
            <div className="ranklist">
              {topDirs.map((d, i) => {
                const pct = stats.total_sessions ? Math.round((d.count / stats.total_sessions) * 100) : 0;
                const tools = toolsOfDir(stats, d.cwd);
                return (
                  <div className="rk" key={d.cwd}>
                    <span className="rk-i num">{i + 1}</span>
                    <span className="rk-nm" title={d.cwd}>{d.display_name}</span>
                    <span className="rk-dots">
                      {tools.map((t) => <span key={t} className="d" style={{ background: `var(--${t})` }} />)}
                    </span>
                    <span className="rk-bar"><span className="rk-bar-f" style={{ width: `${(d.count / dirMax) * 100}%` }} /></span>
                    <span className="rk-v num">{d.count}</span>
                    <span className="rk-p num">{pct}%</span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        <p className="stats-note">纯本地只读聚合 · 不联网 · Token 为按字符估算的约值</p>
      </div>
    </div>
  );
}

function StatsHeader({ onClose }: { onClose: () => void }) {
  return (
    <div className="stats-head">
      <div className="stats-head-title"><IconChart size={15} /> 使用统计</div>
      <button className="iconbtn" title="关闭统计" onClick={onClose}><IconX size={15} /></button>
    </div>
  );
}

/// 某目录用到的工具集合（榜单色点用）。by_tool 是全局的，故按 top_dirs 无法精确到目录；
/// 此处用全局存在的工具集合作近似（保持单一数据源，不额外请求）。
function toolsOfDir(stats: StatsDto, _cwd: string): string[] {
  return stats.by_tool.map((t) => t.tool);
}

// ---- 热力图数据构建 ----
type Heat = { cells: number[]; titles: string[]; months: { col: number; label: string }[] };

/// 把 by_day 映射成「近 HEAT_WEEKS 周 × 7 天」的等级网格（列优先，最右为本周）。
/// 等级 0-4 按当日会话数分桶；并计算每个「月初所在列」的月份标签。
function buildHeat(stats: StatsDto | null): Heat {
  const days = HEAT_WEEKS * 7;
  const cells: number[] = new Array(days).fill(0);
  const titles: string[] = new Array(days).fill("");
  const months: { col: number; label: string }[] = [];
  if (!stats || stats.by_day.length === 0) return { cells, titles, months };

  const counts = new Map(stats.by_day.map((d) => [d.day, d.count]));
  // 以最近一天为锚，往前推 days 天。最近一天用 by_day 末尾（已升序）。
  const lastDay = stats.by_day[stats.by_day.length - 1].day;
  const anchor = new Date(lastDay + "T00:00:00Z");
  const maxCount = Math.max(...stats.by_day.map((d) => d.count));

  let prevMonth = -1;
  for (let i = 0; i < days; i++) {
    // i=0 → 最早；i=days-1 → 最近(锚)。grid 列优先：col=floor(i/7), row=i%7。
    const d = new Date(anchor);
    d.setUTCDate(d.getUTCDate() - (days - 1 - i));
    const key = d.toISOString().slice(0, 10);
    const c = counts.get(key) ?? 0;
    cells[i] = bucket(c, maxCount);
    titles[i] = c > 0 ? `${key} · ${c} 个会话` : key;
    // 月份标签：每列第一天（row 0）若进入新月份，标注该列。
    if (i % 7 === 0) {
      const m = d.getUTCMonth();
      if (m !== prevMonth) {
        months.push({ col: i / 7, label: `${m + 1}月` });
        prevMonth = m;
      }
    }
  }
  return { cells, titles, months };
}

/// 会话数 → 0-4 等级（相对当日峰值分桶）。
function bucket(count: number, max: number): number {
  if (count <= 0) return 0;
  if (max <= 1) return 4;
  const r = count / max;
  if (r > 0.66) return 4;
  if (r > 0.4) return 3;
  if (r > 0.15) return 2;
  return 1;
}

/// 等级 → 青蓝深浅（用 color-mix 复用 accent，保持单 accent 锁）。
function heatColor(level: number): string {
  if (level <= 0) return "var(--panel-3)";
  const mix = [25, 45, 68, 100][level - 1];
  return `color-mix(in srgb, var(--accent) ${mix}%, var(--panel-3))`;
}

export default StatsPanel;
