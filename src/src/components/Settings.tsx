import { useEffect, useState, type ReactNode } from "react";
import type { AppConfig, ProviderConfig, ProviderKind } from "../types";
import {
  getDataDir,
  importNow,
  openPath,
  quitApp,
  updateConfig,
} from "../api";

const KINDS: ProviderKind[] = ["opencode-free", "openrouter", "ollama", "custom"];

export default function Settings({
  config,
  onSaved,
}: {
  config: AppConfig | null;
  onSaved: () => void;
}) {
  const [draft, setDraft] = useState<AppConfig | null>(config);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    setDraft(config);
  }, [config]);

  if (!draft) return null;

  const setGlobal = (patch: Partial<AppConfig>) =>
    setDraft((d) => (d ? { ...d, ...patch } : d));

  const setProvider = (idx: number, patch: Partial<ProviderConfig>) =>
    setDraft((d) =>
      d
        ? {
            ...d,
            providers: d.providers.map((p, i) =>
              i === idx ? { ...p, ...patch } : p
            ),
          }
        : d
    );

  const addProvider = () =>
    setDraft((d) =>
      d
        ? {
            ...d,
            providers: [
              ...d.providers,
              {
                id: `custom-${Date.now()}`,
                kind: "custom",
                enabled: true,
                base_url: "http://127.0.0.1:11434/v1",
                api_key: null,
                priority: d.providers.length,
                model_filter: null,
                extra_models: null,
              },
            ],
          }
        : d
    );

  const removeProvider = (idx: number) =>
    setDraft((d) =>
      d
        ? { ...d, providers: d.providers.filter((_, i) => i !== idx) }
        : d
    );

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setMsg(null);
    try {
      await updateConfig(draft);
      setMsg("Saved & reloaded.");
      onSaved();
    } catch (e) {
      setMsg(`Save failed: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };

  const reimport = async () => {
    setBusy(true);
    setMsg(null);
    try {
      await importNow();
      setMsg("Re-synced models to WorkBuddy.");
    } catch (e) {
      setMsg(`Import failed: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };

  const openFolder = async () => {
    const dir = await getDataDir();
    if (dir) await openPath(dir);
  };

  const openModels = async () => {
    if (draft.workbuddy_models_path) await openPath(draft.workbuddy_models_path);
  };

  return (
    <div className="space-y-5">
      {msg && (
        <div className="glass rounded-xl px-4 py-2 text-sm text-white/80">
          {msg}
        </div>
      )}

      <section className="glass rounded-2xl p-4 space-y-3">
        <h3 className="font-semibold">Gateway</h3>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Bind">
            <input
              className="inp"
              value={draft.bind}
              onChange={(e) => setGlobal({ bind: e.target.value })}
            />
          </Field>
          <Field label="Port">
            <input
              type="number"
              className="inp"
              value={draft.port}
              onChange={(e) =>
                setGlobal({ port: Number(e.target.value) || 0 })
              }
            />
          </Field>
          <Field label="Concurrency">
            <input
              type="number"
              className="inp"
              value={draft.concurrency}
              onChange={(e) =>
                setGlobal({ concurrency: Number(e.target.value) || 1 })
              }
            />
          </Field>
          <Field label="Request timeout (s)">
            <input
              type="number"
              className="inp"
              value={draft.request_timeout_secs}
              onChange={(e) =>
                setGlobal({
                  request_timeout_secs: Number(e.target.value) || 30,
                })
              }
            />
          </Field>
          <Field label="Max retries">
            <input
              type="number"
              className="inp"
              value={draft.max_retries}
              onChange={(e) =>
                setGlobal({ max_retries: Number(e.target.value) || 0 })
              }
            />
          </Field>
          <Field label="Retry base delay (ms)">
            <input
              type="number"
              className="inp"
              value={draft.retry_base_delay_ms}
              onChange={(e) =>
                setGlobal({
                  retry_base_delay_ms: Number(e.target.value) || 0,
                })
              }
            />
          </Field>
        </div>
      </section>

      <section className="glass rounded-2xl p-4 space-y-3">
        <h3 className="font-semibold">Proxy</h3>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={draft.use_system_proxy}
            onChange={(e) =>
              setGlobal({ use_system_proxy: e.target.checked })
            }
          />
          Use system proxy (macOS scutil)
        </label>
        <Field label="Proxy URL (optional)">
          <input
            className="inp"
            placeholder="http://127.0.0.1:7897"
            value={draft.proxy_url ?? ""}
            onChange={(e) =>
              setGlobal({
                proxy_url: e.target.value || null,
              })
            }
          />
        </Field>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={draft.remove_on_exit}
            onChange={(e) =>
              setGlobal({ remove_on_exit: e.target.checked })
            }
          />
          Remove Ferrydock models from WorkBuddy on quit
        </label>
        <Field label="WorkBuddy models.json path">
          <input
            className="inp"
            value={draft.workbuddy_models_path}
            onChange={(e) =>
              setGlobal({ workbuddy_models_path: e.target.value })
            }
          />
        </Field>
      </section>

      <section className="glass rounded-2xl p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="font-semibold">Providers</h3>
          <button className="btn-sm" onClick={addProvider}>
            + Add
          </button>
        </div>
        <div className="space-y-3">
          {draft.providers.map((p, idx) => (
            <div key={p.id} className="rounded-xl bg-black/20 p-3 space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-mono text-xs">{p.id}</span>
                <div className="flex items-center gap-3">
                  <label className="flex items-center gap-1 text-xs">
                    <input
                      type="checkbox"
                      checked={p.enabled}
                      onChange={(e) =>
                        setProvider(idx, { enabled: e.target.checked })
                      }
                    />
                    enabled
                  </label>
                  <button
                    className="text-rose-300 text-xs"
                    onClick={() => removeProvider(idx)}
                  >
                    remove
                  </button>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <Field label="Kind">
                  <select
                    className="inp"
                    value={p.kind}
                    onChange={(e) =>
                      setProvider(idx, {
                        kind: e.target.value as ProviderKind,
                      })
                    }
                  >
                    {KINDS.map((k) => (
                      <option key={k} value={k}>
                        {k}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="Priority">
                  <input
                    type="number"
                    className="inp"
                    value={p.priority}
                    onChange={(e) =>
                      setProvider(idx, {
                        priority: Number(e.target.value) || 0,
                      })
                    }
                  />
                </Field>
                <Field label="Base URL">
                  <input
                    className="inp"
                    value={p.base_url ?? ""}
                    onChange={(e) =>
                      setProvider(idx, {
                        base_url: e.target.value || null,
                      })
                    }
                  />
                </Field>
                <Field label="API key">
                  <input
                    className="inp"
                    placeholder="(optional)"
                    value={p.api_key ?? ""}
                    onChange={(e) =>
                      setProvider(idx, {
                        api_key: e.target.value || null,
                      })
                    }
                  />
                </Field>
              </div>
            </div>
          ))}
        </div>
      </section>

      <div className="flex flex-wrap gap-2">
        <button className="btn" disabled={busy} onClick={save}>
          Save &amp; hot-reload
        </button>
        <button className="btn" disabled={busy} onClick={reimport}>
          Re-sync to WorkBuddy
        </button>
        <button className="btn" onClick={openFolder}>
          Open config folder
        </button>
        <button className="btn" onClick={openModels}>
          Open models.json
        </button>
        <button className="btn" onClick={() => quitApp()}>
          Quit
        </button>
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <div className="text-[10px] uppercase tracking-wider text-white/40 mb-1">
        {label}
      </div>
      {children}
    </label>
  );
}
