import { useState } from "react";
import { useStore } from "../store";
import type { SessionMeta } from "../types";
import { IconSparkle, IconChevron, IconRefresh, IconCopy } from "./icons";

/// AI 会话摘要块：挂在 Reader 头部下方，可折叠。
/// 形态 A（密集块）· 三段式：一句话 / 关键结论 / 涉及代码。
/// 6 状态：已生成·展开 / 空态 / 生成中 / 错误 / 未配置 / 折叠。
export function AiSummary({ session }: { session: SessionMeta }) {
  const { aiConfig, aiSummary, aiSummaryLoading, aiSummaryError, genAiSummary, openSettings, showToast } =
    useStore();
  const [open, setOpen] = useState(false);

  const usable = !!(aiConfig?.enabled && aiConfig.has_key);
  const hasSummary = !!aiSummary && (!!aiSummary.gist || aiSummary.conclusions.length > 0 || aiSummary.files.length > 0);

  // 折叠态头部右侧提示文案。
  const meta = aiSummaryLoading
    ? "生成中…"
    : hasSummary
      ? "已生成，点击展开"
      : usable
        ? "未生成"
        : "未配置";

  const copySummary = async () => {
    if (!aiSummary) return;
    const parts = [aiSummary.gist];
    if (aiSummary.conclusions.length) parts.push("\n关键结论：\n" + aiSummary.conclusions.map((c) => "· " + c).join("\n"));
    if (aiSummary.files.length) parts.push("\n涉及代码：\n" + aiSummary.files.join("\n"));
    try {
      await navigator.clipboard.writeText(parts.join("\n"));
      showToast("已复制摘要");
    } catch {
      showToast("复制失败");
    }
  };

  return (
    <div className={`ai-summary${open ? " open" : ""}`}>
      <div className="ai-sum-head" onClick={() => setOpen((v) => !v)}>
        <span className="spark"><IconSparkle size={14} /></span>
        <span className="label">AI 摘要</span>
        <span className="meta">· {meta}</span>
        <span className="chev"><IconChevron size={13} /></span>
        {hasSummary && usable && (
          <span className="actions" onClick={(e) => e.stopPropagation()}>
            <button className="btn ghost sm" onClick={() => genAiSummary(session, true)} disabled={aiSummaryLoading}>
              <IconRefresh size={12} /> 重新生成
            </button>
            <button className="btn ghost sm" onClick={copySummary}>
              <IconCopy size={12} /> 复制
            </button>
          </span>
        )}
      </div>

      {open && (
        <div className="ai-sum-body">
          {/* 状态：未配置 */}
          {!usable ? (
            <div className="ai-off">
              <span className="ico"><IconSparkle size={14} /></span>
              <span className="txt">
                AI 摘要需要先在「设置」里填写 API 地址、密钥与模型并开启。
                <a onClick={openSettings}> 前往设置 ›</a>
              </span>
            </div>
          ) : aiSummaryLoading ? (
            /* 状态：生成中 */
            <div className="ai-loading">
              <div className="status"><span className="pulse" />正在概括会话要点…</div>
              {[96, 88, 70].map((w) => (
                <div key={w} className="sk" style={{ width: `${w}%` }} />
              ))}
            </div>
          ) : aiSummaryError ? (
            /* 状态：错误 */
            <div className="ai-error">
              <span className="ico">⚠</span>
              <span className="txt"><b>生成失败</b> · {aiSummaryError}</span>
              <button className="btn ghost sm" onClick={() => genAiSummary(session, true)}>重试</button>
            </div>
          ) : hasSummary ? (
            /* 状态：已生成 */
            <div className="ai-sum-inner">
              {aiSummary!.gist && (
                <div className="ai-field">
                  <div className="ft"><IconSparkle size={11} /> 一句话</div>
                  <div className="gist">{aiSummary!.gist}</div>
                </div>
              )}
              {aiSummary!.conclusions.length > 0 && (
                <div className="ai-field">
                  <div className="ft">关键结论</div>
                  <ul>
                    {aiSummary!.conclusions.map((c, i) => (
                      <li key={i}>{c}</li>
                    ))}
                  </ul>
                </div>
              )}
              {aiSummary!.files.length > 0 && (
                <div className="ai-field">
                  <div className="ft">涉及代码</div>
                  <div className="code-chips">
                    {aiSummary!.files.map((f, i) => (
                      <span key={i} className="code-chip">{f}</span>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : (
            /* 状态：空态（已配置未生成） */
            <div className="ai-empty">
              <div className="txt">
                <div className="t1">还没有为这次会话生成摘要</div>
                <div className="t2">点右侧按钮调用你配置的模型概括要点，结果会永久缓存。</div>
              </div>
              <button className="btn primary sm" onClick={() => genAiSummary(session, false)}>
                <IconSparkle size={12} /> 生成摘要
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
