import type { ModelEntry } from "../types";

export default function ModelList({ models }: { models: ModelEntry[] }) {
  if (!models.length)
    return (
      <div className="text-white/50 text-sm">No models registered yet.</div>
    );
  return (
    <div className="glass rounded-2xl p-2 overflow-hidden">
      <table className="w-full text-sm">
        <thead className="text-white/40 text-xs uppercase tracking-wider">
          <tr>
            <th className="text-left px-3 py-2">Model</th>
            <th className="text-left px-3 py-2">Provider</th>
            <th className="text-left px-3 py-2">Capabilities</th>
          </tr>
        </thead>
        <tbody>
          {models.map((m) => (
            <tr key={m.id} className="border-t border-white/5">
              <td className="px-3 py-2 font-mono text-xs break-all">{m.id}</td>
              <td className="px-3 py-2 text-white/70">{m.provider_id}</td>
              <td className="px-3 py-2 space-x-1">
                {m.supports_tools && <span className="tag">tools</span>}
                {m.supports_images && <span className="tag">img</span>}
                {m.supports_reasoning && <span className="tag">reason</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
