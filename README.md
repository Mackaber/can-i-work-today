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

The app starts hidden in the tray. Choose **Open status** from its tray menu (or left-click the tray icon on macOS and Windows) to open the window. Use its native title bar to move or resize it; closing the window hides it in the tray. The window stays open when it loses focus, while the Settings panel closes when you click elsewhere. Add services using the plus button. The Settings menu changes the polling interval (5, 10, 15, or 30 minutes) and lets you edit a custom service registry; selected services and preferences are persisted in the platform's application config directory.

## Service registry

The initial registry is in [`registry.json`](./registry.json). It is embedded in the application at build time so the app has a usable service catalog without a network dependency. Services use `json` or `rss` feeds and define how to interpret them in `parsing_rules`. New registries can be published and wired to a remote update mechanism in a follow-up.

Use **Settings > Edit custom registry** to open the JSON editor in a separate window. Enter a JSON array with the same fields as `registry.json` (`id`, `name`, `category`, `feed_url`, `status_url`, `type`, and `parsing_rules`). An entry with an existing ID replaces that built-in service; an unused ID adds a service. Click **Save** or press Cmd+S / Ctrl+S to apply changes immediately. Closing the editor window keeps unsaved edits until the app quits; reopening it restores the draft. Saved entries are stored in the platform's application config `settings.json` and preserved when other settings are changed. Custom entries saved before parsing rules were introduced remain editable but report unknown until rules are added.

Each `parsing_rules` object requires:

- `items_path`: for JSON, `$` for the root or a dot-separated path such as `$.incidents` or `$.status`, selecting an array or a single object. For RSS, a descendant element path such as `//item` or `//channel/item`.
- `status_path`: a path relative to each item: dot-separated JSON fields such as `status` or `details.state` (`$` selects the item itself), or slash-separated XML element names such as `title` or `details/status`. XML names use local tag names, without namespace prefixes.
- `degraded_regex`: a Rust regular expression matched against each status string, for example `(?i)outage|degraded`. A match reports degraded; valid non-matching strings report healthy.
- Optional JSON `end_path`: a relative field path such as `end` for feeds containing historical incidents. A null or empty string means active; a non-empty string means ended and is skipped. A missing or invalid field reports unknown.

Empty item arrays or RSS feeds report healthy. Missing or empty status strings report unknown unless another item matches the degraded regex. Unsupported paths, invalid regexes, malformed feeds, and missing JSON item paths report unknown; custom rules are validated on save. RSS item paths identify elements by name, so an incorrectly configured path that matches no elements is indistinguishable from an empty feed.

Checks run at the selected interval. Click a service row to open its status source, or use its check button to check it immediately. A service feed that cannot be reached or interpreted is shown as unknown rather than being treated as healthy. The tray dot is red when a tracked service has an active incident, green when checks are healthy, and gray when a result is unknown.

The app sends a notification when a tracked service changes from healthy to having an issue. On macOS, if alerts do not appear, check **System Settings > Notifications** and allow notifications for Can I Work Today. Tauri's desktop permission check does not reflect the macOS notification setting.

## Release builds

Push a `v*` tag to start the GitHub Actions release workflow. It builds a macOS DMG, Windows MSI, and Linux AppImage and attaches them to a draft GitHub release.
