import { useState } from "react";
import { useStore } from "../store";
import { IconDownload, IconCheck } from "./icons";
import type { ExportFormat } from "../types";

const FORMATS: { id: ExportFormat; label: string; sub: string }[] = [
  { id: "md", label: "Markdown", sub: ".md · 适合笔记 / 文档" },
  { id: "html", label: "HTML", sub: ".html · 自包含，双击即看" },
];

export function ExportDialog() {
  const { exportTarget, closeExport, doExport } = useStore();
  const [format, setFormat] = useState<ExportFormat>("md");
  const [includeTools, setIncludeTools] = useState(true);
  const [busy, setBusy] = useState(false);

  if (!exportTarget) return null;

  const run = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await doExport(format, includeTools);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeExport();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") closeExport();
        if (e.key === "Enter") run();
      }}
    >
      <div className="modal" style={{ width: 430 }}>
        <div className="modal-head">
          <h2>
            <span style={{ display: "flex", color: "var(--accent)" }}>
              <IconDownload size={17} />
            </span>
            导出会话
          </h2>
          <p>将当前会话保存为文件，原始记录不会被改动。</p>
        </div>
        <div className="modal-body">
          <div className="term-pick">
            {FORMATS.map((f) => (
              <div
                key={f.id}
                className={`term-opt ${format === f.id ? "sel" : ""}`}
                onClick={() => setFormat(f.id)}
              >
                <span className="d2" />
                <div>
                  <div className="tk">{f.label}</div>
                  <div className="sk2">{f.sub}</div>
                </div>
              </div>
            ))}
          </div>
          <label className="export-opt">
            <input
              type="checkbox"
              checked={includeTools}
              onChange={(e) => setIncludeTools(e.target.checked)}
            />
            <span className="box" aria-hidden>
              {includeTools && <IconCheck size={12} />}
            </span>
            包含工具调用与输出
          </label>
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={closeExport}>
            取消
          </button>
          <button className="btn primary" onClick={run} disabled={busy}>
            <IconDownload size={13} /> {busy ? "导出中…" : "导出"}
          </button>
        </div>
      </div>
    </div>
  );
}
