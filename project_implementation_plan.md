# Project: "Can I Work Today?" - Implementation Plan

## 1. Overview
"Can I Work Today?" is a desktop-first, system-tray utility that monitors the health of cloud infrastructure and SaaS platforms (AWS, GCP, Azure, Google Workspace, etc.). If a critical service the user depends on goes down, the app notifies them that they have the day off. 

## 2. Architecture & Tech Stack
*   **Framework:** Tauri v2
*   **Backend (Core Logic & OS Integration):** Rust
*   **Frontend (UI/Tray Flyout):** React + TypeScript + Tailwind CSS (via Vite)
*   **Cross-Platform Targets:** macOS (.dmg), Windows (.msi/.exe), Linux (.AppImage)
*   **Data Source (Registry):** A static `registry.json` hosted via GitHub Pages containing service definitions and feed URLs.
*   **Local Storage:** `tauri-plugin-store` (for saving the user's selected services).

## 3. UI/UX Flow (System Tray Only)
The application runs entirely in the system tray (no main window).
1.  **Tray Icon:** A simple dot indicating global status (Green = All Clear, Red = Outage/Take the day off).
2.  **Flyout Menu:** Clicking the tray icon opens a small popover window.
3.  **Search/Autocomplete:** A search bar at the top of the flyout allows users to search for services (from the global registry) to add to their monitored list. It shows the Top 10 most common by default when focused.
4.  **Monitored List:** Below the search bar, a list of their actively tracked services and their current real-time status.
5.  **Notifications:** OS-level desktop notifications triggered by the Rust backend when a tracked service changes state.

## 4. Data Models

### 4.1. Remote Registry Schema (`registry.json`)
```json
[
  {
    "id": "aws-us-east-1",
    "name": "AWS (us-east-1)",
    "category": "Cloud Provider",
    "feed_url": "https://status.aws.amazon.com/rss/ec2-us-east-1.rss",
    "type": "rss"
  },
  {
    "id": "google-workspace",
    "name": "Google Workspace",
    "category": "SaaS",
    "feed_url": "https://www.google.com/appsstatus/dashboard/incidents.json",
    "type": "json"
  }
]
```

### 4.2. Local User State (Tauri Store)
```json
{
  "monitored_services": ["aws-us-east-1", "google-workspace"],
  "poll_interval_minutes": 10
}
```

## 5. Implementation Phases

### Phase 1: Setup & Scaffolding
*   [ ] Initialize Tauri project with React/Vite template.
*   [ ] Configure `tauri.conf.json` to disable the main window on launch and enable the system tray.
*   [ ] Install necessary Rust crates: `reqwest` (HTTP), `serde_json`, `roxmltree` (XML/RSS parsing), `tokio` (async).
*   [ ] Install frontend dependencies: `tailwindcss`, `lucide-react` (icons), `downshift` or similar for the autocomplete component.

### Phase 2: Rust Backend (The Core Engine)
*   [ ] Implement a service struct and JSON parser to fetch and cache the static `registry.json` from a public URL.
*   [ ] Create a polling loop using `tokio::time::interval` that runs every 10 minutes.
*   [ ] Implement XML/RSS and JSON parsers for the feeds to determine if a service is "healthy" or "degraded".
*   [ ] Integrate `tauri::SystemTray` to render the tray icon. Add logic to swap the icon asset between Green and Red based on polling results.
*   [ ] Implement OS notifications using Tauri's native notification API when a state transitions from Healthy -> Degraded.

### Phase 3: Inter-Process Communication (IPC)
*   [ ] Create Tauri commands (Rust functions exposed to the frontend):
    *   `get_registry()`: Returns the full list of available services.
    *   `get_status()`: Returns the real-time health of the user's tracked services.
    *   `add_service(id)`: Adds a service to local tracking.
    *   `remove_service(id)`: Removes a service.

### Phase 4: Frontend UI (The Flyout)
*   [ ] Design the flyout layout: Search bar header, scrollable list of tracked services body, minimal footer (Settings/Quit).
*   [ ] Implement the Autocomplete search bar. It should fetch `get_registry()`, filter based on user input, and allow selecting a service to add to tracking.
*   [ ] Render the monitored list dynamically, polling `get_status()` via Tauri IPC every minute while the flyout is open to ensure UI freshness.

### Phase 5: CI/CD Packaging
*   [ ] Create `.github/workflows/release.yml`.
*   [ ] Configure Tauri GitHub Action to compile and upload `.dmg`, `.msi`, and `.AppImage` binaries automatically on repository tags.