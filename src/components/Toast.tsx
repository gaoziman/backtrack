import { useStore } from "../store";
import { IconCheck } from "./icons";

export function Toast() {
  const toast = useStore((s) => s.toast);
  return (
    <div className={`toast ${toast ? "show" : ""}`}>
      <span className="chk"><IconCheck size={11} /></span>
      <span>{toast}</span>
    </div>
  );
}
