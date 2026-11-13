use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Notify;

mod parsing;
use parsing::{json_path_segments, parse_json, parse_rss, xml_path_segments};

const DEFAULT_POLL_INTERVAL_MINUTES: u64 = 10;
const REGISTRY_JSON: &str = include_str!("../../registry.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Service {
    id: String,
    name: String,
    category: String,
    feed_url: String,
    status_url: String,
    #[serde(rename = "type")]
    feed_type: String,
    #[serde(default)]
    parsing_rules: ParsingRules,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ParsingRules {
    items_path: String,
    status_path: String,
    degraded_regex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Health {
    Healthy,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
struct ServiceStatus {
    id: String,
    name: String,
    category: String,
    state: Health,
    detail: String,
    checked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize)]
struct UserConfig {
    #[serde(default)]
    monitored_services: Vec<String>,
    #[serde(default = "default_poll_interval")]
    poll_interval_minutes: u64,
    #[serde(default)]
    custom_registry: Vec<Service>,
}

struct AppState {
    registry: Mutex<Vec<Service>>,
    custom_registry: Mutex<Vec<Service>>,
    monitored: Mutex<HashSet<String>>,
    statuses: Mutex<HashMap<String, ServiceStatus>>,
    poll_interval_minutes: Mutex<u64>,
    poll_interval_changed: Notify,
}

fn default_poll_interval() -> u64 {
    DEFAULT_POLL_INTERVAL_MINUTES
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create settings directory: {error}"))?;
    Ok(dir.join("settings.json"))
}

fn load_user_config(app: &AppHandle) -> Result<UserConfig, String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(UserConfig {
            monitored_services: Vec::new(),
            poll_interval_minutes: DEFAULT_POLL_INTERVAL_MINUTES,
            custom_registry: Vec::new(),
        });
    }
    let contents =
        fs::read_to_string(&path).map_err(|error| format!("Could not read settings: {error}"))?;
    let mut config: UserConfig = serde_json::from_str(&contents)
        .map_err(|error| format!("Could not parse settings: {error}"))?;
    for service in &config.custom_registry {
        if service.parsing_rules.is_missing() {
            eprintln!(
                "Custom service {} needs parsing_rules; its status will be unknown until updated",
                service.id
            );
        }
    }
    let default_registry: Vec<Service> =
        serde_json::from_str(REGISTRY_JSON).map_err(|error| error.to_string())?;
    let registry = merge_registry(&default_registry, &config.custom_registry)?;
    let known_ids: HashSet<&str> = registry.iter().map(|service| service.id.as_str()).collect();
    config
        .monitored_services
        .retain(|id| known_ids.contains(id.as_str()));
    if ![5, 10, 15, 30].contains(&config.poll_interval_minutes) {
        eprintln!(
            "Ignoring unsupported saved poll interval: {} minutes",
            config.poll_interval_minutes
        );
        config.poll_interval_minutes = DEFAULT_POLL_INTERVAL_MINUTES;
    }
    Ok(config)
}

fn save_user_config(
    app: &AppHandle,
    monitored: &HashSet<String>,
    poll_interval_minutes: u64,
    custom_registry: &[Service],
) -> Result<(), String> {
    let path = config_path(app)?;
    let mut ids: Vec<&String> = monitored.iter().collect();
    ids.sort_unstable();
    let contents = serde_json::to_string_pretty(&UserConfig {
        monitored_services: ids.into_iter().cloned().collect(),
        poll_interval_minutes,
        custom_registry: custom_registry.to_vec(),
    })
    .map_err(|error| format!("Could not serialize settings: {error}"))?;
    let temporary_path = path.with_extension("json.tmp");
    fs::write(&temporary_path, contents)
        .map_err(|error| format!("Could not write settings: {error}"))?;
    fs::rename(&temporary_path, &path).map_err(|error| format!("Could not save settings: {error}"))
}

impl ParsingRules {
    fn is_missing(&self) -> bool {
        self.items_path.is_empty()
            && self.status_path.is_empty()
            && self.degraded_regex.is_empty()
            && self.end_path.is_none()
    }

    fn validate(&self, feed_type: &str) -> Result<(), String> {
        if self.items_path.trim().is_empty()
            || self.status_path.trim().is_empty()
            || self.degraded_regex.trim().is_empty()
        {
            return Err(
                "parsing_rules requires non-empty items_path, status_path, and degraded_regex"
                    .to_string(),
            );
        }
        Regex::new(&self.degraded_regex)
            .map_err(|error| format!("Invalid degraded_regex: {error}"))?;
        if feed_type == "json" {
            json_path_segments(&self.items_path, true)?;
            json_path_segments(&self.status_path, false)?;
            if let Some(path) = &self.end_path {
                json_path_segments(path, false)?;
            }
        } else {
            xml_path_segments(&self.items_path, true)?;
            xml_path_segments(&self.status_path, false)?;
            if self.end_path.is_some() {
                return Err("end_path is only supported for JSON feeds".to_string());
            }
        }
        Ok(())
    }
}

fn validate_custom_registry(services: &[Service], allow_legacy: bool) -> Result<(), String> {
    let mut ids = HashSet::new();
    for service in services {
        if service.id.trim().is_empty()
            || service.name.trim().is_empty()
            || service.category.trim().is_empty()
            || service.feed_url.trim().is_empty()
            || service.status_url.trim().is_empty()
        {
            return Err("Each custom service must have non-empty id, name, category, feed_url, and status_url fields".to_string());
        }
        if !["rss", "json"].contains(&service.feed_type.as_str()) {
            return Err(format!(
                "Unsupported feed type for {}: {} (expected rss or json)",
                service.id, service.feed_type
            ));
        }
        if !(allow_legacy && service.parsing_rules.is_missing()) {
            service
                .parsing_rules
                .validate(&service.feed_type)
                .map_err(|error| format!("Invalid parsing rules for {}: {error}", service.id))?;
        }
        if !ids.insert(&service.id) {
            return Err(format!(
                "Custom registry contains duplicate service id: {}",
                service.id
            ));
        }
    }
    Ok(())
}

fn merge_registry(defaults: &[Service], custom: &[Service]) -> Result<Vec<Service>, String> {
    validate_custom_registry(custom, true)?;
    let mut merged = defaults.to_vec();
    for service in custom {
        if let Some(existing) = merged.iter_mut().find(|item| item.id == service.id) {
            *existing = service.clone();
        } else {
            merged.push(service.clone());
        }
    }
    Ok(merged)
}

fn status_for(service: &Service, state: Health, detail: String) -> ServiceStatus {
    ServiceStatus {
        id: service.id.clone(),
        name: service.name.clone(),
        category: service.category.clone(),
        state,
        detail,
        checked_at: Some(Utc::now()),
    }
}

#[tauri::command]
fn get_registry(state: State<'_, AppState>) -> Result<Vec<Service>, String> {
    state
        .registry
        .lock()
        .map(|registry| registry.clone())
        .map_err(|_| "Service registry is unavailable".to_string())
}

#[tauri::command]
fn get_custom_registry(state: State<'_, AppState>) -> Result<Vec<Service>, String> {
    state
        .custom_registry
        .lock()
        .map(|registry| registry.clone())
        .map_err(|_| "Custom service registry is unavailable".to_string())
}

#[tauri::command]
fn set_custom_registry(
    app: AppHandle,
    state: State<'_, AppState>,
    services: Vec<Service>,
) -> Result<(), String> {
    validate_custom_registry(&services, false)?;
    let defaults: Vec<Service> =
        serde_json::from_str(REGISTRY_JSON).map_err(|error| error.to_string())?;
    let registry = merge_registry(&defaults, &services)?;
    let mut monitored = state
        .monitored
        .lock()
        .map_err(|_| "Service list is unavailable".to_string())?;
    let poll_interval = state
        .poll_interval_minutes
        .lock()
        .map_err(|_| "Poll interval setting is unavailable".to_string())?;
    let mut custom_registry = state
        .custom_registry
        .lock()
        .map_err(|_| "Custom service registry is unavailable".to_string())?;
    let known_ids: HashSet<&str> = registry.iter().map(|service| service.id.as_str()).collect();
    let mut updated_monitored = monitored.clone();
    updated_monitored.retain(|id| known_ids.contains(id.as_str()));
    save_user_config(&app, &updated_monitored, *poll_interval, &services)?;

    *state
        .registry
        .lock()
        .map_err(|_| "Service registry is unavailable".to_string())? = registry.clone();
    *custom_registry = services;
    *monitored = updated_monitored.clone();

    let mut statuses = state
        .statuses
        .lock()
        .map_err(|_| "Status data is unavailable".to_string())?;
    *statuses = registry
        .iter()
        .filter(|service| updated_monitored.contains(&service.id))
        .map(|service| {
            (
                service.id.clone(),
                ServiceStatus {
                    id: service.id.clone(),
                    name: service.name.clone(),
                    category: service.category.clone(),
                    state: Health::Unknown,
                    detail: "Waiting for the first check".to_string(),
                    checked_at: None,
                },
            )
        })
        .collect();
    drop(statuses);
    drop(monitored);
    drop(custom_registry);
    drop(poll_interval);
    update_tray_icon(&app, &state);
    state.poll_interval_changed.notify_one();
    if let Err(error) = app.emit("registry-changed", ()) {
        eprintln!("Could not notify windows of registry change: {error}");
    }
    Ok(())
}

#[tauri::command]
async fn open_registry_editor(app: AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window("registry-editor") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            &app,
            "registry-editor",
            WebviewUrl::App("index.html".into()),
        )
        .title("Custom Registry")
        .inner_size(760.0, 620.0)
        .min_inner_size(480.0, 360.0)
        .center()
        .visible(false)
        .build()
        .map_err(|error| format!("Could not create registry editor: {error}"))?,
    };
    window
        .show()
        .map_err(|error| format!("Could not show registry editor: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Could not focus registry editor: {error}"))
}

#[tauri::command]
fn get_poll_interval(state: State<'_, AppState>) -> Result<u64, String> {
    state
        .poll_interval_minutes
        .lock()
        .map(|minutes| *minutes)
        .map_err(|_| "Poll interval setting is unavailable".to_string())
}

#[tauri::command]
fn set_poll_interval(
    app: AppHandle,
    state: State<'_, AppState>,
    minutes: u64,
) -> Result<(), String> {
    if ![5, 10, 15, 30].contains(&minutes) {
        return Err("Poll interval must be 5, 10, 15, or 30 minutes".to_string());
    }
    let monitored = state
        .monitored
        .lock()
        .map_err(|_| "Service list is unavailable".to_string())?;
    let mut poll_interval = state
        .poll_interval_minutes
        .lock()
        .map_err(|_| "Poll interval setting is unavailable".to_string())?;
    let custom_registry = state
        .custom_registry
        .lock()
        .map_err(|_| "Custom service registry is unavailable".to_string())?;
    save_user_config(&app, &monitored, minutes, &custom_registry)?;
    *poll_interval = minutes;
    state.poll_interval_changed.notify_one();
    Ok(())
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>, String> {
    let registry = state
        .registry
        .lock()
        .map_err(|_| "Service registry is unavailable".to_string())?
        .clone();
    let monitored = state
        .monitored
        .lock()
        .map_err(|_| "Service list is unavailable".to_string())?
        .clone();
    let statuses = state
        .statuses
        .lock()
        .map_err(|_| "Status data is unavailable".to_string())?;
    Ok(registry
        .iter()
        .filter(|service| monitored.contains(&service.id))
        .map(|service| {
            statuses
                .get(&service.id)
                .cloned()
                .unwrap_or_else(|| ServiceStatus {
                    id: service.id.clone(),
                    name: service.name.clone(),
                    category: service.category.clone(),
                    state: Health::Unknown,
                    detail: "Waiting for the first check".to_string(),
                    checked_at: None,
                })
        })
        .collect())
}

#[tauri::command]
async fn add_service(
    app: AppHandle,
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), String> {
    let registry = state
        .registry
        .lock()
        .map_err(|_| "Service registry is unavailable".to_string())?
        .clone();
    let service = registry
        .iter()
        .find(|service| service.id == service_id)
        .cloned()
        .ok_or_else(|| format!("Unknown service: {service_id}"))?;
    {
        let mut monitored = state
            .monitored
            .lock()
            .map_err(|_| "Service list is unavailable".to_string())?;
        let poll_interval = state
            .poll_interval_minutes
            .lock()
            .map_err(|_| "Poll interval setting is unavailable".to_string())?;
        let custom_registry = state
            .custom_registry
            .lock()
            .map_err(|_| "Custom service registry is unavailable".to_string())?;
        let mut updated = monitored.clone();
        updated.insert(service_id.clone());
        save_user_config(&app, &updated, *poll_interval, &custom_registry)?;
        *monitored = updated;
    }
    let status = check_service(&Client::new(), &service).await;
    state
        .statuses
        .lock()
        .map_err(|_| "Status data is unavailable".to_string())?
        .insert(service.id, status);
    update_tray_icon(&app, &state);
    Ok(())
}

#[tauri::command]
fn remove_service(
    app: AppHandle,
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), String> {
    {
        let mut monitored = state
            .monitored
            .lock()
            .map_err(|_| "Service list is unavailable".to_string())?;
        let poll_interval = state
            .poll_interval_minutes
            .lock()
            .map_err(|_| "Poll interval setting is unavailable".to_string())?;
        let custom_registry = state
            .custom_registry
            .lock()
            .map_err(|_| "Custom service registry is unavailable".to_string())?;
        let mut updated = monitored.clone();
        updated.remove(&service_id);
        save_user_config(&app, &updated, *poll_interval, &custom_registry)?;
        *monitored = updated;
    }
    state
        .statuses
        .lock()
        .map_err(|_| "Status data is unavailable".to_string())?
        .remove(&service_id);
    update_tray_icon(&app, &state);
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn open_service_source(
    state: State<'_, AppState>,
    app: AppHandle,
    service_id: String,
) -> Result<(), String> {
    let service = state
        .registry
        .lock()
        .map_err(|_| "Service registry is unavailable".to_string())?
        .iter()
        .find(|service| service.id == service_id)
        .cloned()
        .ok_or_else(|| format!("Unknown service: {service_id}"))?;
    app.opener()
        .open_url(&service.status_url, None::<&str>)
        .map_err(|error| format!("Could not open status source: {error}"))
}

async fn check_service(client: &Client, service: &Service) -> ServiceStatus {
    let response = match client
        .get(&service.feed_url)
        .header("User-Agent", "CanIWorkToday/0.1 (+https://github.com)")
        .timeout(Duration::from_secs(15))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return status_for(
                service,
                Health::Unknown,
                format!("Could not reach status feed: {error}"),
            );
        }
    };
    if !response.status().is_success() {
        return status_for(
            service,
            Health::Unknown,
            format!("Status feed returned HTTP {}", response.status()),
        );
    }
    let body = match response.text().await {
        Ok(body) => body,
        Err(error) => {
            return status_for(
                service,
                Health::Unknown,
                format!("Could not read status feed: {error}"),
            )
        }
    };

    let result = match service.feed_type.as_str() {
        "rss" => parse_rss(&body, &service.parsing_rules),
        "json" => parse_json(&body, &service.parsing_rules),
        other => Err(format!("Unsupported feed type: {other}")),
    };
    match result {
        Ok((Health::Healthy, detail)) => status_for(service, Health::Healthy, detail),
        Ok((Health::Degraded, detail)) => status_for(service, Health::Degraded, detail),
        Ok((Health::Unknown, detail)) => status_for(service, Health::Unknown, detail),
        Err(error) => status_for(
            service,
            Health::Unknown,
            format!("Could not interpret status feed: {error}"),
        ),
    }
}

fn make_icon(color: [u8; 3]) -> Image<'static> {
    let mut pixels = vec![0_u8; 16 * 16 * 4];
    for y in 0..16 {
        for x in 0..16 {
            let dx = x as i32 - 7;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 30 {
                let index = (y * 16 + x) * 4;
                pixels[index] = color[0];
                pixels[index + 1] = color[1];
                pixels[index + 2] = color[2];
                pixels[index + 3] = 255;
            }
        }
    }
    Image::new_owned(pixels, 16, 16)
}

fn update_tray_icon(app: &AppHandle, state: &AppState) {
    let monitored = match state.monitored.lock() {
        Ok(monitored) => monitored.clone(),
        Err(error) => {
            eprintln!("Could not read monitored services for tray icon: {error}");
            return;
        }
    };
    let color = match state.statuses.lock() {
        Ok(statuses) => {
            if monitored.iter().any(|id| {
                statuses
                    .get(id)
                    .is_some_and(|status| status.state == Health::Degraded)
            }) {
                [235, 82, 82]
            } else if monitored.iter().any(|id| {
                statuses
                    .get(id)
                    .is_none_or(|status| status.state == Health::Unknown)
            }) {
                [139, 151, 171]
            } else {
                [73, 198, 129]
            }
        }

        Err(error) => {
            eprintln!("Could not read status for tray icon: {error}");
            return;
        }
    };
    if let Some(tray) = app.tray_by_id("status") {
        if let Err(error) = tray.set_icon(Some(make_icon(color))) {
            eprintln!("Could not update tray icon: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        merge_registry, parse_json, parse_rss, validate_custom_registry, Health, ParsingRules,
        Service, REGISTRY_JSON,
    };

    fn rules(items: &str, status: &str, regex: &str) -> ParsingRules {
        ParsingRules {
            items_path: items.to_string(),
            status_path: status.to_string(),
            degraded_regex: regex.to_string(),
            end_path: None,
        }
    }

    fn service(id: &str, name: &str) -> Service {
        Service {
            id: id.to_string(),
            name: name.to_string(),
            category: "Custom".to_string(),
            feed_url: "https://example.com/feed".to_string(),
            status_url: "https://example.com/status".to_string(),
            feed_type: "json".to_string(),
            parsing_rules: rules("$.incidents", "status", "(?i)outage"),
        }
    }

    #[test]
    fn custom_registry_replaces_matching_ids_and_adds_new_ids() {
        let defaults = vec![
            service("existing", "Built-in"),
            service("unchanged", "Unchanged"),
        ];
        let custom = vec![
            service("existing", "Customized"),
            service("new", "New service"),
        ];

        let merged = merge_registry(&defaults, &custom).unwrap();
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].name, "Customized");
        assert_eq!(merged[1].name, "Unchanged");
        assert_eq!(merged[2].id, "new");
    }

    #[test]
    fn custom_registry_rejects_duplicate_ids_and_unsupported_feed_types() {
        let defaults = vec![service("default", "Default")];
        assert!(merge_registry(
            &defaults,
            &[
                service("duplicate", "First"),
                service("duplicate", "Second")
            ]
        )
        .is_err());

        let mut unsupported = service("custom", "Custom");
        unsupported.feed_type = "xml".to_string();
        assert!(merge_registry(&defaults, &[unsupported]).is_err());
    }

    #[test]
    fn registry_rules_are_valid_and_cover_all_services() {
        let services: Vec<Service> = serde_json::from_str(REGISTRY_JSON).unwrap();
        validate_custom_registry(&services, false).unwrap();
        assert!(services
            .iter()
            .all(|service| !service.parsing_rules.is_missing()));
    }

    #[test]
    fn built_in_rules_interpret_representative_feed_shapes() {
        let services: Vec<Service> = serde_json::from_str(REGISTRY_JSON).unwrap();
        let cases = [
            ("aws-us-east-1", "<rss><channel><item><title>Service disruption</title></item></channel></rss>", "<rss><channel/></rss>"),
            ("github", r#"{"status":{"indicator":"major"}}"#, r#"{"status":{"indicator":"none"}}"#),
            ("google-cloud", r#"[{"end":null,"most_recent_update":{"status":"SERVICE_OUTAGE"}}]"#, r#"[{"end":"2026-01-01","most_recent_update":{"status":"SERVICE_OUTAGE"}}]"#),
            ("google-workspace", r#"[{"end":null,"most_recent_update":{"status":"SERVICE_DISRUPTION"}}]"#, r#"[{"end":"2026-01-01","most_recent_update":{"status":"SERVICE_DISRUPTION"}}]"#),
            ("cloudflare", r#"{"status":{"indicator":"minor"}}"#, r#"{"status":{"indicator":"none"}}"#),
            ("slack", r#"{"status":"active","active_incidents":[]}"#, r#"{"status":"ok","active_incidents":[]}"#),
            ("openai", r#"{"status":{"indicator":"critical"}}"#, r#"{"status":{"indicator":"none"}}"#),
            ("vercel", r#"{"status":{"indicator":"major"}}"#, r#"{"status":{"indicator":"none"}}"#),
            ("microsoft-365", "<rss><channel><item><description>User impact: Users cannot connect</description></item></channel></rss>", "<rss><channel/></rss>"),
            ("azure", "<rss><channel><item><title>Service outage</title></item></channel></rss>", "<rss><channel/></rss>"),
        ];
        assert_eq!(cases.len(), services.len());
        for (id, degraded, healthy) in cases {
            let service = services.iter().find(|service| service.id == id).unwrap();
            let parse = if service.feed_type == "rss" {
                parse_rss
            } else {
                parse_json
            };
            assert_eq!(
                parse(degraded, &service.parsing_rules).unwrap().0,
                Health::Degraded,
                "{id}"
            );
            assert_eq!(
                parse(healthy, &service.parsing_rules).unwrap().0,
                Health::Healthy,
                "{id}"
            );
        }
    }

    #[test]
    fn json_rules_select_status_dynamically() {
        let status_rules = rules("$.status", "indicator", "(?i)^(major|minor|critical)$");
        assert_eq!(
            parse_json(
                r#"{"status":{"indicator":"none","description":"All Systems Operational"}}"#,
                &status_rules,
            )
            .unwrap()
            .0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(
                r#"{"status":{"indicator":"major","description":"Major outage"}}"#,
                &status_rules
            )
            .unwrap()
            .0,
            Health::Degraded
        );
        let custom = rules("$.events", "details.condition", "(?i)red");
        assert_eq!(
            parse_json(
                r#"{"events":[{"details":{"condition":"RED"}},{"details":{"condition":"green"}}]}"#,
                &custom
            )
            .unwrap()
            .0,
            Health::Degraded
        );
        assert_eq!(
            parse_json(r#"{"events":[]}"#, &custom).unwrap().0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(r#"{"events":[{"details":{}}]}"#, &custom)
                .unwrap()
                .0,
            Health::Unknown
        );
        assert_eq!(
            parse_json(
                r#"{"events":[{},{"details":{"condition":"red"}}]}"#,
                &custom
            )
            .unwrap()
            .0,
            Health::Degraded
        );
    }

    #[test]
    fn incident_feeds_ignore_resolved_incidents() {
        let mut rules = rules("$", "state", "(?i)outage");
        rules.end_path = Some("end".to_string());
        let body =
            r#"[{"state":"outage","end":null},{"state":"outage","end":"2026-01-01T00:00:00Z"}]"#;
        let result = parse_json(body, &rules).unwrap();
        assert_eq!(result.0, Health::Degraded);
        assert_eq!(result.1, "outage");

        assert_eq!(
            parse_json(
                r#"[{"state":"outage","end":"2026-01-01T00:00:00Z"}]"#,
                &rules
            )
            .unwrap()
            .0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(r#"[{"state":"outage"}]"#, &rules).unwrap().0,
            Health::Unknown
        );
    }

    #[test]
    fn unknown_json_and_non_rss_xml_do_not_report_healthy() {
        let json = rules("$.incidents", "status", "outage");
        let rss = rules("//item", "title", "outage");
        assert!(parse_json(r#"{"message":"not a status feed"}"#, &json).is_err());
        assert!(parse_json(r#"{"incidents":"bad"}"#, &json).is_err());
        assert!(parse_rss("<root><item><title>unrelated</title></item></root>", &rss).is_err());
        assert!(parse_rss("<rss>", &rss).is_err());
    }

    #[test]
    fn rss_rules_select_configured_nodes() {
        let rss = rules("//channel/item", "details/state", "(?i)outage");
        assert_eq!(
            parse_rss("<rss><channel><title>Status</title></channel></rss>", &rss)
                .unwrap()
                .0,
            Health::Healthy
        );
        assert_eq!(
            parse_rss("<rss><channel><item><details><state>ok</state></details></item><item><details><state>OUTAGE</state></details></item></channel></rss>", &rss)
                .unwrap()
                .0,
            Health::Degraded
        );
        assert_eq!(
            parse_rss(
                "<rss><channel><item><title>Missing state</title></item></channel></rss>",
                &rss
            )
            .unwrap()
            .0,
            Health::Unknown
        );
        let rss = rules(
            "//item",
            "status",
            "(?i)servicedegradation|serviceinterruption",
        );
        assert_eq!(
            parse_rss("<rss><channel><item><title>All is well</title><status>ServiceDegradation</status></item></channel></rss>", &rss).unwrap().0,
            Health::Degraded
        );
        assert_eq!(
            parse_rss("<rss><channel><item><title>ServiceInterruption</title><status>Operational</status></item></channel></rss>", &rss).unwrap().0,
            Health::Healthy
        );
    }

    #[test]
    fn invalid_rules_are_rejected_and_legacy_rules_remain_editable() {
        let mut custom = service("custom", "Custom");
        custom.parsing_rules.degraded_regex = "(".to_string();
        assert!(validate_custom_registry(&[custom.clone()], false).is_err());
        custom.parsing_rules = ParsingRules::default();
        assert!(validate_custom_registry(&[custom.clone()], false).is_err());
        assert!(merge_registry(&[], &[custom.clone()]).is_ok());
        assert!(parse_json("{}", &custom.parsing_rules).is_err());
        let legacy = r#"{"id":"legacy","name":"Legacy","category":"Custom","feed_url":"https://example.com","status_url":"https://example.com","type":"json"}"#;
        let parsed: Service = serde_json::from_str(legacy).unwrap();
        assert!(parsed.parsing_rules.is_missing());
        assert!(serde_json::to_string(&parsed)
            .unwrap()
            .contains("parsing_rules"));
        assert!(rules("$.incidents[0]", "status", "x")
            .validate("json")
            .is_err());
        assert!(rules("//item", "../status", "x").validate("rss").is_err());
    }
}

async fn check_and_update_service(
    app: &AppHandle,
    state: &AppState,
    client: &Client,
    service: &Service,
) -> Result<ServiceStatus, String> {
    let next = check_service(client, service).await;
    let previous = state
        .statuses
        .lock()
        .map_err(|_| "Status data is unavailable".to_string())?
        .insert(service.id.clone(), next.clone())
        .map(|status| status.state);
    if previous == Some(Health::Healthy) && next.state == Health::Degraded {
        if let Err(error) = app
            .notification()
            .builder()
            .title(format!("{} is having trouble", service.name))
            .body(&next.detail)
            .show()
        {
            eprintln!("Could not show outage notification: {error}");
        }
    }
    update_tray_icon(app, state);
    Ok(next)
}

#[tauri::command]
async fn check_service_now(
    app: AppHandle,
    state: State<'_, AppState>,
    service_id: String,
) -> Result<ServiceStatus, String> {
    if !state
        .monitored
        .lock()
        .map_err(|_| "Service list is unavailable".to_string())?
        .contains(&service_id)
    {
        return Err(format!("Service is not being monitored: {service_id}"));
    }
    let service = state
        .registry
        .lock()
        .map_err(|_| "Service registry is unavailable".to_string())?
        .iter()
        .find(|service| service.id == service_id)
        .cloned()
        .ok_or_else(|| format!("Unknown service: {service_id}"))?;
    check_and_update_service(&app, &state, &Client::new(), &service).await
}

async fn poll_services(app: AppHandle) {
    let client = match Client::builder().user_agent("CanIWorkToday/0.1").build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("Could not create HTTP client: {error}");
            return;
        }
    };
    loop {
        let state = app.state::<AppState>();
        let monitored_ids = match state.monitored.lock() {
            Ok(monitored) => monitored.clone(),
            Err(error) => {
                eprintln!("Could not read monitored services: {error}");
                continue;
            }
        };
        let registry = match state.registry.lock() {
            Ok(registry) => registry.clone(),
            Err(error) => {
                eprintln!("Could not read service registry: {error}");
                continue;
            }
        };
        for service in registry
            .iter()
            .filter(|service| monitored_ids.contains(&service.id))
        {
            if let Err(error) = check_and_update_service(&app, &state, &client, service).await {
                eprintln!("Could not update service status: {error}");
            }
        }
        let minutes = match state.poll_interval_minutes.lock() {
            Ok(minutes) => *minutes,
            Err(error) => {
                eprintln!("Could not read poll interval setting: {error}");
                DEFAULT_POLL_INTERVAL_MINUTES
            }
        };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(minutes * 60)) => {}
            _ = state.poll_interval_changed.notified() => {}
        }
    }
}

fn show_status_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("Could not find status window");
        return;
    };
    if let Err(error) = window.show() {
        eprintln!("Could not show status window: {error}");
        return;
    }
    if let Err(error) = window.unminimize() {
        eprintln!("Could not restore status window: {error}");
    }
    if let Err(error) = window.set_focus() {
        eprintln!("Could not focus status window: {error}");
    }
}

pub fn run() {
    let default_registry: Vec<Service> =
        serde_json::from_str(REGISTRY_JSON).expect("registry.json must be valid");
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let config = load_user_config(&handle).map_err(std::io::Error::other)?;
            let registry = merge_registry(&default_registry, &config.custom_registry)
                .map_err(std::io::Error::other)?;
            let monitored: HashSet<String> = config.monitored_services.into_iter().collect();
            let poll_interval_minutes = config.poll_interval_minutes;
            let statuses = registry
                .iter()
                .filter(|service| monitored.contains(&service.id))
                .map(|service| {
                    (
                        service.id.clone(),
                        ServiceStatus {
                            id: service.id.clone(),
                            name: service.name.clone(),
                            category: service.category.clone(),
                            state: Health::Unknown,
                            detail: "Waiting for the first check".to_string(),
                            checked_at: None,
                        },
                    )
                })
                .collect();
            app.manage(AppState {
                registry: Mutex::new(registry),
                custom_registry: Mutex::new(config.custom_registry),
                monitored: Mutex::new(monitored),
                statuses: Mutex::new(statuses),
                poll_interval_minutes: Mutex::new(poll_interval_minutes),
                poll_interval_changed: Notify::new(),
            });

            let show = MenuItem::with_id(app, "show", "Open status", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("status")
                .icon(make_icon([139, 151, 171]))
                .tooltip("Can I Work Today?")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_status_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_status_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(poll_services(app_handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" || window.label() == "registry-editor" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        eprintln!("Could not hide {} window: {error}", window.label());
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_registry,
            get_poll_interval,
            set_poll_interval,
            get_status,
            check_service_now,
            add_service,
            remove_service,
            get_custom_registry,
            set_custom_registry,
            open_registry_editor,
            open_service_source,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running Can I Work Today");
}
