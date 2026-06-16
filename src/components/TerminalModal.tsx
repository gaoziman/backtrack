import { useState } from "react";
import { useStore } from "../store";
import { IconPlay, IconTerminal } from "./icons";

const TERMINALS = [
  { id: "iTerm", label: "iTerm2", sub: "默认终端" },
  { id: "Terminal", label: "Terminal", sub: "系统自带" },
  { id: "Warp", label: "Warp", sub: "回退到 Terminal" },
];

export function TerminalModal() {
  const { terminalModal, closeTerminal, doResume } = useStore();
  const [picked, setPicked] = useState("iTerm");

  if (!terminalModal) return null;
  const s = terminalModal;

  return (
    <div className="scrim" onClick={(e) => { if (e.target === e.currentTarget) closeTerminal(); }}>
      <div className="modal">
        <div className="modal-head">
          <h2><IconTerminal size={17} /> 在终端恢复会话</h2>
          <p>将打开终端，自动进入会话目录并执行 resume 命令。</p>
        </div>
        <div className="modal-body">
          <div className="term-preview">
            <span className="pr">➜</span> <span className="cmt"># 自动注入以下命令</span><br />
            <span className="cm">cd</span> <span className="ac">{s.cwd}</span><br />
            <span className="cm">{s.resume_command}</span>
          </div>
          <div className="term-pick">
            {TERMINALS.map((t) => (
              <div
                key={t.id}
                className={`term-opt ${picked === t.id ? "sel" : ""}`}
                onClick={() => setPicked(t.id)}
              >
                <span className="d2" />
                <div>
                  <div className="tk">{t.label}</div>
                  <div className="sk2">{t.sub}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={() => closeTerminal()}>取消</button>
          <button className="btn primary" onClick={() => doResume(s, picked)}>
            <IconPlay size={13} /> 打开终端并恢复
          </button>
        </div>
      </div>
    </div>
  );
}
