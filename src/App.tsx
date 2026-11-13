import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { Check, ChevronDown, CircleAlert, Cloud, LoaderCircle, Minus, Plus, Search, Settings2, X } from "lucide-react";

type Service = {
  id: string;
  name: string;
  category: string;
  feed_url: string;
  type: "rss" | "json";
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
  const [error, setError] = useState("");
  const [isFocused, setIsFocused] = useState(true);
  const searchRef = useRef<HTMLInputElement>(null);

  async function hideFlyout() {
    try {
      await getCurrentWindow().hide();
    } catch (reason) {
      setError(`Could not close the flyout: ${String(reason)}`);
    }
  }

  const refresh = useCallback(async () => {
    try {
      const current = await invoke<ServiceStatus[]>("get_status");
      setStatuses(current);
      setError("");
    } catch (reason) {
      setError(`Could not load service status: ${String(reason)}`);
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void Promise.all([
      invoke<Service[]>("get_registry").then((services) => {
        if (!cancelled) setRegistry(services);
      }),
      refresh(),
      invoke<number>("get_poll_interval").then((minutes) => {
        if (!cancelled) setPollInterval(minutes);
      }),
      getCurrentWindow().onFocusChanged(({ payload }) => setIsFocused(payload)),
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
  }, [refresh]);

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
        <button className="icon-button close-button" aria-label="Close window" onClick={() => void hideFlyout()}>
          <X size={16} />
        </button>
      </header>

      <section className={`summary-card ${hasOutage ? "summary-card--outage" : ""}`}>
        <div className={`summary-icon ${hasOutage ? "summary-icon--outage" : allHealthy ? "summary-icon--healthy" : ""}`}>
          {hasOutage ? <CircleAlert size={20} /> : allHealthy ? <Check size={20} /> : <LoaderCircle className="spin" size={20} />}
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
              <article className="service-row" key={status.id}>
                <span className={`status-dot status-dot--${status.state}`} aria-label={stateLabels[status.state]} />
                <div className="service-info">
                  <div className="service-name-line">
                    <h3>{status.name}</h3>
                    <span className={`status-label status-label--${status.state}`}>{stateLabels[status.state]}</span>
                  </div>
                  <p>{status.detail}</p>
                </div>
                <button
                  className="icon-button remove-button"
                  aria-label={`Stop monitoring ${status.name}`}
                  title={`Stop monitoring ${status.name}`}
                  disabled={busy}
                  onClick={() => void removeService(status.id)}
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
          <div className="settings-wrap">
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
                <small>Changes are saved on this device.</small>
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
