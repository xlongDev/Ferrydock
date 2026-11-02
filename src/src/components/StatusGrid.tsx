import type { HealthResponse, StatusResponse } from "../types";

export default function StatusGrid({
  status,
  health,
}: {
  status: StatusResponse | null;
  health: HealthResponse | null;
}) {
  if (!status) return null;
  return (
    <div className="space-y-4">
      <div className="glass rounded-2xl p-4 flex items-center justify-between">
        <div>
          <div className="text-xs uppercase tracking-wider text-white/50">
            Gateway
          </div>
          <div className="text-lg font-semibold">
            {health?.status === "ok" ? "Running" : "—"}
          </div>
        </div>
        <div className="flex gap-6 text-right">
          <Stat label="Models" value={String(status.models.length)} />
          <Stat label="Providers" value={String(status.providers.length)} />
          <Stat label="Version" value={health?.version ?? "—"} />
        </div>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {status.providers.map((p) => (
          <div key={p.id} className="glass rounded-2xl p-4">
            <div className="flex items-center justify-between">
              <div className="font-mono text-sm">{p.id}</div>
              <Dot ok={p.healthy} />
            </div>
            <div className="mt-3 grid grid-cols-2 gap-2 text-xs text-white/70">
              <Stat label="Success" value={String(p.success)} />
              <Stat label="Error" value={String(p.error)} />
              <Stat
                label="Latency"
                value={`${p.latency_ms.toFixed(0)}ms`}
              />
              <Stat
                label="Circuit"
                value={p.circuit_open ? "OPEN" : "closed"}
              />
            </div>
            {p.last_error && (
              <div className="mt-2 text-[11px] text-rose-300/80 break-all">
                {p.last_error}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wider text-white/40">
        {label}
      </div>
      <div className="font-medium">{value}</div>
    </div>
  );
}

function Dot({ ok }: { ok: boolean }) {
  return (
    <span
      className={`inline-block h-2.5 w-2.5 rounded-full ${
        ok ? "bg-emerald-400" : "bg-rose-400"
      }`}
    />
  );
}
