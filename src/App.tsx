import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, ChevronDown, CircleAlert, Cloud, LoaderCircle, Minus, MessageCircleQuestion, Plus, RefreshCw, Search, Settings2, X } from "lucide-react";

type Service = {
  id: string;
  name: string;
  category: string;
  feed_url: string;
  status_url: string;
  type: "rss" | "json";
  parsing_rules: {
    items_path: string;
    status_path: string;
    degraded_regex: string;
    end_path?: string;
  };
};

type Health = "healthy" | "degraded" | "unknown";

type ServiceStatus = {
  id: string;
  name: string;
  category: string;
  state: Health;
  detail: string;
  checked_at: string | null;
};

const stateLabels: Record<Health, string> = {
  healthy: "Operational",
  degraded: "Service issue",
  unknown: "Status unknown",
};

function App() {
  const [registry, setRegistry] = useState<Service[]>([]);
  const [statuses, setStatuses] = useState<ServiceStatus[]>([]);
  const [query, setQuery] = useState("");
  const [focused, setFocused] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [pollInterval, setPollInterval] = useState(10);
  const [busy, setBusy] = useState(false);
  const [checkingServices, setCheckingServices] = useState<Set<string>>(() => new Set());
  const [error, setError] = useState("");
  const [isFocused, setIsFocused] = useState(true);
  const searchRef = useRef<HTMLInputElement>(null);
  const settingsRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(async () => {
    try {
      const current = await invoke<ServiceStatus[]>("get_status");
      setStatuses(current);
      setError("");
    } catch (reason) {
      setError(`Could not load service status: ${String(reason)}`);
    }
  }, []);

  const refreshRegistry = useCallback(async () => {
    try {
      const services = await invoke<Service[]>("get_registry");
      setRegistry(services);
    } catch (reason) {
      setError(`Could not load service registry: ${String(reason)}`);
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void Promise.all([
      refreshRegistry(),
      refresh(),
      invoke<number>("get_poll_interval").then((minutes) => {
        if (!cancelled) setPollInterval(minutes);
      }),
      getCurrentWindow().onFocusChanged(({ payload }) => {
        setIsFocused(payload);
        if (payload) {
          void refreshRegistry();
          void refresh();
        } else {
          setSettingsOpen(false);
        }
      }),
    ]).then(([, , , stopListening]) => {
      if (cancelled) stopListening();
      else unlisten = stopListening;
    }).catch((reason) => {
      if (!cancelled) setError(`Could not start the status monitor: ${String(reason)}`);
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh, refreshRegistry]);

  useEffect(() => {
    if (!settingsOpen) return;
    function dismissSettings(event: PointerEvent) {
      if (event.target instanceof Node && !settingsRef.current?.contains(event.target)) {
        setSettingsOpen(false);
      }
    }
    document.addEventListener("pointerdown", dismissSettings);
    return () => document.removeEventListener("pointerdown", dismissSettings);
  }, [settingsOpen]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen("registry-changed", () => {
      void refreshRegistry();
      void refresh();
    }).then((stopListening) => {
      if (cancelled) stopListening();
      else unlisten = stopListening;
    }).catch((reason) => {
      if (!cancelled) setError(`Could not watch registry changes: ${String(reason)}`);
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh, refreshRegistry]);

  useEffect(() => {
    if (!isFocused) return;
    const timer = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(timer);
  }, [isFocused, refresh]);

  const monitoredIds = useMemo(() => new Set(statuses.map((status) => status.id)), [statuses]);
  const filteredServices = useMemo(() => {
    const available = registry.filter((service) => !monitoredIds.has(service.id));
    const term = query.trim().toLocaleLowerCase();
    const matches = term
      ? available.filter((service) =>
          `${service.name} ${service.category}`.toLocaleLowerCase().includes(term),
        )
      : available;
    return matches.slice(0, 10);
  }, [monitoredIds, query, registry]);

  const hasOutage = statuses.some((status) => status.state === "degraded");
  const allHealthy = statuses.length > 0 && statuses.every((status) => status.state === "healthy");

  async function addService(id: string) {
    setBusy(true);
    try {
      await invoke("add_service", { serviceId: id });
      setQuery("");
      setSearchOpen(false);
      await refresh();
    } catch (reason) {
      setError(`Could not add service: ${String(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function removeService(id: string) {
    setBusy(true);
    try {
      await invoke("remove_service", { serviceId: id });
      await refresh();
    } catch (reason) {
      setError(`Could not remove service: ${String(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function openServiceSource(status: ServiceStatus) {
    try {
      await invoke("open_service_source", { serviceId: status.id });
    } catch (reason) {
      setError(`Could not open ${status.name}'s status source: ${String(reason)}`);
    }
  }

  async function checkServiceNow(status: ServiceStatus) {
    setCheckingServices((current) => new Set(current).add(status.id));
    try {
      const updated = await invoke<ServiceStatus>("check_service_now", { serviceId: status.id });
      setStatuses((current) => current.map((item) => item.id === updated.id ? updated : item));
      setError("");
    } catch (reason) {
      setError(`Could not check ${status.name}: ${String(reason)}`);
    } finally {
      setCheckingServices((current) => {
        const next = new Set(current);
        next.delete(status.id);
        return next;
      });
    }
  }

  async function updatePollInterval(minutes: number) {
    setBusy(true);
    try {
      await invoke("set_poll_interval", { minutes });
      setPollInterval(minutes);
    } catch (reason) {
      setError(`Could not update the polling interval: ${String(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function openRegistryEditor() {
    setBusy(true);
    try {
      await invoke("open_registry_editor");
      setError("");
    } catch (reason) {
      setError(`Could not open custom registry: ${String(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function quit() {
    try {
      await invoke("quit_app");
    } catch (reason) {
      setError(`Could not quit the app: ${String(reason)}`);
    }
  }

  const allUnknown = statuses.length > 0 && statuses.every((status) => status.state === "unknown");
  const firstCheckPending =
    allUnknown && statuses.every((status) => status.checked_at === null);
  const headline = hasOutage
    ? "Maybe take the day off."
    : statuses.length === 0
      ? "Add your work essentials."
      : allHealthy
        ? "You’re good to go."
        : allUnknown
          ? firstCheckPending
            ? "Checking your services…"
            : "We can’t confirm service status."
          : "Checking your services…";
  const summary = hasOutage
    ? "At least one service you rely on is having trouble."
    : statuses.length === 0
      ? "Pick the services you want us to keep an eye on."
      : allHealthy
        ? "All your tracked services are up and running."
        : allUnknown
          ? firstCheckPending
            ? "We’re checking the services you depend on."
            : "A status feed couldn’t be checked. Try again shortly."
          : "We’re checking the services you depend on.";

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-mark" aria-hidden="true">
          <Cloud size={18} strokeWidth={2.2} />
        </div>
        <span className="brand-name">CAN I WORK TODAY</span>
      </header>

      <section className={`summary-card ${hasOutage ? "summary-card--outage" : ""}`}>
        <div className={`summary-icon ${hasOutage ? "summary-icon--outage" : allHealthy ? "summary-icon--healthy" : ""}`}>
          {hasOutage ? <CircleAlert size={20} /> : allHealthy ? <Check size={20} /> : <MessageCircleQuestion size={20} />}
        </div>
        <div>
          <p className="eyebrow">{hasOutage ? "STATUS ALERT" : "YOUR WORKDAY CHECK"}</p>
          <h1>{headline}</h1>
          <p className="summary-copy">{summary}</p>
        </div>
      </section>

      <section className="services-section">
        <div className="section-heading">
          <div>
            <h2>Your services</h2>
            <span className="service-count">{statuses.length} monitored</span>
          </div>
          <button
            className={`add-toggle ${searchOpen ? "add-toggle--open" : ""}`}
            onClick={() => {
              setSearchOpen((open) => !open);
              setQuery("");
              window.setTimeout(() => searchRef.current?.focus(), 0);
            }}
            aria-label={searchOpen ? "Close service search" : "Add a service"}
            aria-expanded={searchOpen}
          >
            {searchOpen ? <X size={16} /> : <Plus size={16} />}
          </button>
        </div>

        {searchOpen && (
          <div className="search-wrap">
            <Search size={16} className="search-icon" />
            <input
              ref={searchRef}
              value={query}
              placeholder="Search services…"
              onChange={(event) => {
                setQuery(event.target.value);
                setFocused(true);
              }}
              onFocus={() => setFocused(true)}
              onBlur={() => window.setTimeout(() => setFocused(false), 120)}
              onKeyDown={(event) => {
                if (event.key === "Escape") setSearchOpen(false);
                if (event.key === "Enter" && filteredServices[0]) void addService(filteredServices[0].id);
              }}
              aria-label="Search services to monitor"
            />
            <ChevronDown size={14} className="search-chevron" />
            {focused && (
              <div className="search-results">
                {filteredServices.length === 0 ? (
                  <p className="empty-search">{query ? "No matching services." : "You’re monitoring every service."}</p>
                ) : (
                  filteredServices.map((service) => (
                    <button
                      className="search-result"
                      key={service.id}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => void addService(service.id)}
                      disabled={busy}
                    >
                      <span className="result-dot" />
                      <span className="result-text">
                        <span>{service.name}</span>
                        <small>{service.category}</small>
                      </span>
                      <Plus size={15} />
                    </button>
                  ))
                )}
              </div>
            )}
          </div>
        )}

        <div className="service-list">
          {statuses.length === 0 ? (
            <div className="empty-state">
              <div className="empty-illustration"><Cloud size={23} /></div>
              <p>No services yet</p>
              <span>Add the tools and platforms you rely on.</span>
              {!searchOpen && (
                <button
                  className="text-button"
                  onClick={() => {
                    setSearchOpen(true);
                    window.setTimeout(() => searchRef.current?.focus(), 0);
                  }}
                >
                  Add your first service <Plus size={14} />
                </button>
              )}
            </div>
          ) : (
            statuses.map((status) => (
              <article
                className="service-row"
                key={status.id}
                role="link"
                tabIndex={0}
                aria-label={`Open ${status.name}'s status source in your browser`}
                onClick={() => void openServiceSource(status)}
                onKeyDown={(event) => {
                  if (event.target === event.currentTarget && event.key === "Enter") {
                    event.preventDefault();
                    void openServiceSource(status);
                  }
                }}
              >
                <span className={`status-dot status-dot--${status.state}`} aria-label={stateLabels[status.state]} />
                <span className="service-open">
                  <span className="service-info">
                    <span className="service-name-line">
                      <span className="service-name">{status.name}</span>
                      <span className={`status-label status-label--${status.state}`}>{stateLabels[status.state]}</span>
                    </span>
                    <span className="service-detail">{status.detail}</span>
                  </span>
                </span>
                <button
                  type="button"
                  className="icon-button check-button"
                  aria-label={`Check ${status.name} now`}
                  title={`Check ${status.name} now`}
                  disabled={checkingServices.has(status.id)}
                  onClick={(event) => {
                    event.stopPropagation();
                    void checkServiceNow(status);
                  }}
                  onKeyDown={(event) => event.stopPropagation()}
                >
                  {checkingServices.has(status.id)
                    ? <LoaderCircle size={15} className="spin" />
                    : <RefreshCw size={14} />}
                </button>
                <button
                  className="icon-button remove-button"
                  aria-label={`Stop monitoring ${status.name}`}
                  title={`Stop monitoring ${status.name}`}
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    void removeService(status.id);
                  }}
                  onKeyDown={(event) => event.stopPropagation()}
                >
                  <Minus size={16} />
                </button>
              </article>
            ))
          )}
        </div>
      </section>

      {error && <div className="error-banner" role="alert">{error}</div>}

      <footer className="footer">
        <span className="refresh-note"><span className="live-dot" /> Updates every {pollInterval} minutes</span>
        <div className="footer-actions">
          <div className="settings-wrap" ref={settingsRef}>
            <button
              className="footer-button"
              onClick={() => setSettingsOpen((open) => !open)}
              aria-label="Settings"
              aria-expanded={settingsOpen}
            >
              <Settings2 size={15} />
              <span>Settings</span>
            </button>
            {settingsOpen && (
              <div className="settings-panel">
                <label htmlFor="poll-interval">Check for updates</label>
                <select
                  id="poll-interval"
                  value={pollInterval}
                  disabled={busy}
                  onChange={(event) => void updatePollInterval(Number(event.target.value))}
                >
                  {[5, 10, 15, 30].map((minutes) => (
                    <option key={minutes} value={minutes}>Every {minutes} minutes</option>
                  ))}
                </select>
                <button
                  className="settings-save"
                  type="button"
                  disabled={busy}
                  onClick={() => void openRegistryEditor()}
                >
                  Edit custom registry…
                </button>
                <small>Settings are saved on this device.</small>
              </div>
            )}
          </div>
          <span className="footer-divider" />
          <button className="footer-button quit-button" onClick={() => void quit()}>Quit</button>
        </div>
      </footer>
    </main>
  );
}

export default App;
