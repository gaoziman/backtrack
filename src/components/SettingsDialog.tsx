import { useEffect, useState } from "react";
import { useStore } from "../store";
import { api } from "../api";
import { IconSettings, IconEye, IconEyeOff, IconCheck } from "./icons";

export function SettingsDialog() {
  const { settingsOpen, closeSettings, aiConfig, saveAiConfig } = useStore();
  const [enabled, setEnabled] = useState(false);
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("claude-opus-4-8");
  const [showKey, setShowKey] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);

  // 打开时用已存配置初始化表单（key 不回显明文，留空＝不修改）。
  useEffect(() => {
    if (settingsOpen && aiConfig) {
      setEnabled(aiConfig.enabled);
      setBaseUrl(aiConfig.base_url);
      setModel(aiConfig.model);
      setApiKey("");
      setShowKey(false);
      setTestResult(null);
    }
  }, [settingsOpen, aiConfig]);

  if (!settingsOpen) return null;

  const hasKey = aiConfig?.has_key ?? false;
  const fieldsDisabled = !enabled;

  const runTest = async () => {
    if (testing) return;
    setTesting(true);
    setTestResult(null);
    try {
      await api.testAiConnection(baseUrl, apiKey, model);
      setTestResult({ ok: true, msg: "连接成功" });
    } catch (e) {
      setTestResult({ ok: false, msg: String(e) });
    } finally {
      setTesting(false);
    }
  };

  const save = () => saveAiConfig({ enabled, baseUrl, apiKey, model });

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeSettings();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") closeSettings();
      }}
    >
      <div className="modal" style={{ width: 480 }} role="dialog" aria-modal="true" aria-label="AI 标题设置">
        <div className="modal-head">
          <h2>
            <span style={{ display: "flex", color: "var(--accent)" }}>
              <IconSettings size={17} />
            </span>
            AI 标题设置
          </h2>
          <p>
            开启后，点开的会话内容会发送到你配置的 API 地址用于生成精炼标题。默认关闭，纯本地。
          </p>
        </div>
        <div className="modal-body">
          <label className="export-opt" style={{ marginBottom: 14 }}>
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
            <span className="box" aria-hidden>
              {enabled && <IconCheck size={12} />}
            </span>
            启用 AI 标题概括
          </label>

          <div className={`ai-fields ${fieldsDisabled ? "disabled" : ""}`}>
            <label className="ai-field">
              <span className="ai-label">API 地址</span>
              <input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://xxx/v1/messages"
                spellCheck={false}
              />
            </label>

            <label className="ai-field">
              <span className="ai-label">API Key</span>
              <div className="ai-key-row">
                <input
                  type={showKey ? "text" : "password"}
                  value={apiKey}
                  placeholder={hasKey ? "已配置（留空则不修改）" : "sk-..."}
                  onChange={(e) => setApiKey(e.target.value)}
                  spellCheck={false}
                  autoComplete="off"
                />
                <button
                  type="button"
                  className="iconbtn"
                  aria-label={showKey ? "隐藏密钥" : "显示密钥"}
                  onClick={() => setShowKey((v) => !v)}
                >
                  {showKey ? <IconEyeOff size={15} /> : <IconEye size={15} />}
                </button>
              </div>
              <span className="ai-hint">密钥仅保存在本机，不会上传或提交到代码库。</span>
            </label>

            <label className="ai-field">
              <span className="ai-label">模型</span>
              <input value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} />
            </label>

            <div className="ai-test-row">
              <button className="btn ghost" onClick={runTest} disabled={testing}>
                {testing ? "测试中…" : "测试连接"}
              </button>
              {testResult && (
                <span
                  className="ai-test-result"
                  style={{ color: testResult.ok ? "var(--good)" : "var(--danger)" }}
                >
                  {testResult.ok ? "✓ " : "✗ "}
                  {testResult.msg}
                </span>
              )}
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={closeSettings}>
            取消
          </button>
          <button className="btn primary" onClick={save}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
