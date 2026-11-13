# Can I Work Today?

A lightweight desktop app that watches the cloud and SaaS services you rely on and alerts you when one reports an active incident. The app lives in the system tray; click its icon to open the status flyout.

## Development

### Prerequisites

- Node.js 20 or newer with npm
- Rust stable
- Tauri v2 platform prerequisites for your operating system

Install the frontend dependencies and launch the app:

```sh
npm install
npm run tauri dev
```

The app starts hidden in the tray. Choose **Open status** from its tray menu (or left-click the tray icon) to open the flyout. Add services using the plus button. The Settings menu changes the polling interval (5, 10, 15, or 30 minutes) and lets you edit a custom service registry; selected services and preferences are persisted in the platform's application config directory.

## Service registry

The initial registry is in [`registry.json`](./registry.json). It is embedded in the application at build time so the app has a usable service catalog without a network dependency. Add `json` services that expose a Statuspage-compatible `status.json` endpoint, or `rss` services with an RSS status feed. New registries can be published and wired to a remote update mechanism in a follow-up.

Use **Settings > Edit custom registry** to open the JSON editor in a separate window. Enter a JSON array with the same fields as `registry.json` (`id`, `name`, `category`, `feed_url`, `status_url`, and `type`, which must be `json` or `rss`). An entry with an existing ID replaces that built-in service; an unused ID adds a service. Click **Save** or press Cmd+S / Ctrl+S to apply changes immediately. Closing the editor window keeps unsaved edits until the app quits; reopening it restores the draft. Saved entries are stored in the platform's application config `settings.json` and preserved when other settings are changed.

Checks run at the selected interval. Click a service row to open its status source, or use its check button to check it immediately. A service feed that cannot be reached or interpreted is shown as unknown rather than being treated as healthy. The tray dot is red when a tracked service has an active incident, green when checks are healthy, and gray when a result is unknown.

The app sends a notification when a tracked service changes from healthy to having an issue. On macOS, if alerts do not appear, check **System Settings > Notifications** and allow notifications for Can I Work Today. Tauri's desktop permission check does not reflect the macOS notification setting.

## Release builds

Push a `v*` tag to start the GitHub Actions release workflow. It builds a macOS DMG, Windows MSI, and Linux AppImage and attaches them to a draft GitHub release.
