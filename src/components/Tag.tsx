import type { Tool } from "../types";

export function Tag({ tool }: { tool: Tool }) {
  return (
    <span className={`tag ${tool}`}>
      <span className="d" />
      {tool === "claude" ? "Claude" : "Codex"}
    </span>
  );
}
