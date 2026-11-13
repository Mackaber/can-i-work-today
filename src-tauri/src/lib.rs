use chrono::{DateTime, Utc};
use reqwest::Client;
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

fn validate_custom_registry(services: &[Service]) -> Result<(), String> {
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
    validate_custom_registry(custom)?;
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

    let result = if service.feed_type == "rss" {
        parse_rss(&body)
    } else {
        parse_json(&body)
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

fn parse_json(body: &str) -> Result<(Health, String), String> {
    let value: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;

    if let Some(indicator) = value
        .get("status")
        .and_then(|status| status.get("indicator"))
        .and_then(Value::as_str)
    {
        let description = value
            .get("status")
            .and_then(|status| status.get("description"))
            .and_then(Value::as_str)
            .unwrap_or(indicator);
        return Ok((indicator_health(indicator), description.to_string()));
    }

    if let Some(status) = value.get("status").and_then(Value::as_str) {
        let health = indicator_health(status);
        if health != Health::Unknown {
            return Ok((health, status.to_string()));
        }
    }

    let has_incident_list = match &value {
        Value::Array(_) => true,
        Value::Object(object) => ["incidents", "activeIncidents", "active_incidents"]
            .iter()
            .any(|key| object.get(*key).is_some_and(Value::is_array)),
        _ => false,
    };
    if !has_incident_list {
        return Err("unrecognized JSON status format".to_string());
    }

    let mut active_incidents = Vec::new();
    let mut unknown_incident = false;
    collect_active_incidents(&value, &mut active_incidents, &mut unknown_incident);
    if !active_incidents.is_empty() {
        Ok((
            Health::Degraded,
            active_incidents
                .into_iter()
                .take(2)
                .collect::<Vec<_>>()
                .join("; "),
        ))
    } else if unknown_incident {
        Ok((
            Health::Unknown,
            "Incident status could not be interpreted".to_string(),
        ))
    } else {
        Ok((Health::Healthy, "No active incidents reported".to_string()))
    }
}

fn collect_active_incidents(value: &Value, incidents: &mut Vec<String>, unknown: &mut bool) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_active_incidents(value, incidents, unknown);
            }
        }
        Value::Object(object) => {
            let ended = ["end", "endTime", "end_time"].iter().any(|key| {
                object
                    .get(*key)
                    .is_some_and(|end| !end.is_null() && end.as_str() != Some(""))
            });
            let status = object
                .get("status")
                .or_else(|| object.get("state"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let status_health = indicator_health(status);
            let severity = object
                .get("severity")
                .or_else(|| object.get("impact"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let severity_health = indicator_health(severity);
            if !ended && (status_health == Health::Degraded || severity_health == Health::Degraded)
            {
                let summary = object
                    .get("name")
                    .or_else(|| object.get("title"))
                    .or_else(|| object.get("summary"))
                    .and_then(Value::as_str)
                    .unwrap_or("Active incident");
                incidents.push(summary.to_string());
                return;
            }
            if !ended
                && (object.contains_key("status")
                    || object.contains_key("state")
                    || object.contains_key("severity")
                    || object.contains_key("impact"))
                && status_health == Health::Unknown
                && severity_health == Health::Unknown
            {
                *unknown = true;
            }
            for child in object.values() {
                collect_active_incidents(child, incidents, unknown);
            }
        }
        _ => {}
    }
}

fn indicator_health(indicator: &str) -> Health {
    let value = indicator.to_ascii_lowercase();
    if value.contains("no active")
        || value.contains("no incident")
        || value.contains("no known issue")
    {
        return Health::Healthy;
    }
    if [
        "operational",
        "healthy",
        "none",
        "resolved",
        "ok",
        "up",
        "no incidents",
        "available",
    ]
    .iter()
    .any(|word| value == *word || value.contains(word))
    {
        Health::Healthy
    } else if [
        "degraded",
        "minor",
        "major",
        "critical",
        "outage",
        "disruption",
        "partial",
        "investigating",
        "identified",
        "monitoring",
        "ongoing",
        "active",
        "high",
        "error rate",
        "latency",
        "unavailable",
        "service impact",
        "experiencing issues",
        "down",
    ]
    .iter()
    .any(|word| value.contains(word))
    {
        Health::Degraded
    } else {
        Health::Unknown
    }
}

fn parse_rss(body: &str) -> Result<(Health, String), String> {
    let document = Document::parse(body).map_err(|error| error.to_string())?;
    if !document
        .descendants()
        .any(|node| node.has_tag_name("channel"))
    {
        return Err("RSS channel is missing".to_string());
    }
    let items: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("item"))
        .collect();
    let mut active = Vec::new();
    let mut unknown = false;
    for item in items {
        let title = item
            .children()
            .find(|node| node.has_tag_name("title"))
            .and_then(|node| node.text())
            .unwrap_or("Service incident");
        let description = item
            .children()
            .find(|node| node.has_tag_name("description"))
            .and_then(|node| node.text())
            .unwrap_or("");
        let combined = format!("{title} {description}");
        let lowered = combined.to_ascii_lowercase();
        if ["resolved", "completed", "closed", "fully restored"]
            .iter()
            .any(|word| lowered.contains(word))
        {
            continue;
        }
        if indicator_health(&combined) == Health::Degraded {
            active.push(title.trim().to_string());
        } else if indicator_health(&combined) == Health::Unknown {
            unknown = true;
        }
    }
    if !active.is_empty() {
        Ok((
            Health::Degraded,
            active.into_iter().take(2).collect::<Vec<_>>().join("; "),
        ))
    } else if unknown {
        Ok((
            Health::Unknown,
            "Incident status could not be interpreted".to_string(),
        ))
    } else {
        Ok((Health::Healthy, "No active incidents reported".to_string()))
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
    use super::{merge_registry, parse_json, parse_rss, Health, Service};

    fn service(id: &str, name: &str) -> Service {
        Service {
            id: id.to_string(),
            name: name.to_string(),
            category: "Custom".to_string(),
            feed_url: "https://example.com/feed".to_string(),
            status_url: "https://example.com/status".to_string(),
            feed_type: "json".to_string(),
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
    fn statuspage_indicator_is_respected() {
        assert_eq!(
            parse_json(
                r#"{"status":{"indicator":"none","description":"All Systems Operational"}}"#
            )
            .unwrap()
            .0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(r#"{"status":{"indicator":"major_outage","description":"Major outage"}}"#)
                .unwrap()
                .0,
            Health::Degraded
        );
    }

    #[test]
    fn incident_feeds_ignore_resolved_incidents() {
        let body = r#"[{"status":"SERVICE_OUTAGE","end":null,"title":"API outage"},{"status":"SERVICE_OUTAGE","end":"2026-01-01T00:00:00Z","title":"Old outage"}]"#;
        let result = parse_json(body).unwrap();
        assert_eq!(result.0, Health::Degraded);
        assert_eq!(result.1, "API outage");

        assert_eq!(
            parse_json(r#"[{"status":"SERVICE_OUTAGE","end":"2026-01-01T00:00:00Z"}]"#)
                .unwrap()
                .0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(r#"[{"status":"SERVICE_OUTAGE","endTime":"2026-01-01T00:00:00Z"}]"#)
                .unwrap()
                .0,
            Health::Healthy
        );
        assert_eq!(
            parse_json(r#"[{"status":"unexpected","title":"Unknown incident"}]"#)
                .unwrap()
                .0,
            Health::Unknown
        );
    }

    #[test]
    fn unknown_json_and_non_rss_xml_do_not_report_healthy() {
        assert!(parse_json(r#"{"message":"not a status feed"}"#).is_err());
        assert!(parse_rss("<root><item><title>unrelated</title></item></root>").is_err());
    }

    #[test]
    fn rss_without_incidents_is_healthy() {
        assert_eq!(
            parse_rss("<rss><channel><title>Status</title></channel></rss>")
                .unwrap()
                .0,
            Health::Healthy
        );
        assert_eq!(
            parse_rss("<rss><channel><item><title>Scheduled event</title></item></channel></rss>")
                .unwrap()
                .0,
            Health::Unknown
        );
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
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if let Err(error) = window.show() {
                                eprintln!("Could not show status window: {error}");
                            }
                            if let Err(error) = window.set_focus() {
                                eprintln!("Could not focus status window: {error}");
                            }
                        }
                    }
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
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            if let Err(error) = window.show() {
                                eprintln!("Could not show status window: {error}");
                            }
                            if let Err(error) = window.set_focus() {
                                eprintln!("Could not focus status window: {error}");
                            }
                        }
                    }
                })
                .build(app)?;

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(poll_services(app_handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Focused(false)) {
                if let Err(error) = window.hide() {
                    eprintln!("Could not hide status window: {error}");
                }
            } else if window.label() == "registry-editor" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        eprintln!("Could not hide registry editor: {error}");
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
