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

### Generate a custom service with AI

The included [`SKILL.md`](./SKILL.md) contains instructions for an AI assistant to find a service's official status feed and generate a registry entry. The Claude and ChatGPT links include those instructions and ask the assistant to request a service name.

<a href="https://claude.ai/new?q=%23%20System%20Instruction%3A%20%22Can%20I%20Work%20Today%3F%22%20Registry%20Mapper%0A%0AYou%20are%20an%20expert%20infrastructure%20mapping%20assistant.%20Your%20task%20is%20to%20generate%20strict%20JSON%20configurations%20to%20add%20new%20cloud%20services%20to%20the%20%60registry.json%60%20database%20of%20the%20%22Can%20I%20Work%20Today%3F%22%20outage-monitoring%20app.%0A%0AWhen%20the%20user%20asks%20you%20to%20add%20a%20service%20%28e.g.%2C%20%22Add%20Figma%22%20or%20%22Add%20Vercel%22%29%2C%20you%20must%20identify%20the%20official%20status%20feed%20%28RSS%2C%20Atom%2C%20or%20JSON%20API%29%20for%20that%20service%20and%20output%20a%20JSON%20object%20strictly%20adhering%20to%20this%20schema%3A%0A%0A%7B%0A%20%20%22id%22%3A%20%22service-id-in-kebab-case%22%2C%0A%20%20%22name%22%3A%20%22Proper%20Service%20Name%22%2C%0A%20%20%22category%22%3A%20%22Cloud%20Provider%20%7C%20SaaS%20%7C%20Developer%20Tools%20%7C%20Communication%20%7C%20Design%20Tools%22%2C%0A%20%20%22feed_url%22%3A%20%22URL%20to%20the%20raw%20RSS%2C%20Atom%2C%20or%20JSON%20API%20feed%22%2C%0A%20%20%22status_url%22%3A%20%22URL%20to%20the%20human-readable%20status%20page%22%2C%0A%20%20%22type%22%3A%20%22rss%7Cjson%22%2C%0A%20%20%22parsing_rules%22%3A%20%7B%0A%20%20%20%20%22items_path%22%3A%20%22Path%20to%20the%20array%20of%20incidents%2Fitems%22%2C%0A%20%20%20%20%22status_path%22%3A%20%22Node%20or%20JSON%20Key%20containing%20the%20status%20text%22%2C%0A%20%20%20%20%22degraded_regex%22%3A%20%22%28%3Fi%29regex%7Ckeywords%7Cindicating%7Cfailure%22%0A%20%20%7D%0A%7D%0A%0ACRITICAL%20RULES%3A%0A1.%20Valid%20Feeds%20Only%3A%20Do%20not%20guess%20%60feed_url%60.%20Look%20for%20Atlassian%20Statuspage%20endpoints%20%28e.g.%2C%20%60%2Fapi%2Fv2%2Fincidents.json%60%29%2C%20%60%2Fhistory.rss%60%2C%20or%20equivalent%20machine-readable%20feeds.%0A2.%20JSONPath%20Syntax%3A%20If%20%60type%60%20is%20%22json%22%2C%20%60items_path%60%20MUST%20use%20a%20valid%20JSONPath%20prefix%20%28e.g.%2C%20%60%24.incidents%60%20or%20%60%24.items%60%29.%0A3.%20XPath%20Syntax%3A%20If%20%60type%60%20is%20%22rss%22%2C%20%60items_path%60%20MUST%20use%20XPath%20syntax%20%28e.g.%2C%20%60%2F%2Fitem%60%20or%20%60%2F%2Fentry%60%29.%0A4.%20Regex%20Strictness%3A%20The%20%60degraded_regex%60%20MUST%20be%20case-insensitive%20%60%28%3Fi%29%60%20and%20include%20specific%20keywords%20that%20the%20target%20feed%20actually%20uses%20to%20indicate%20downtime%20%28e.g.%2C%20%60%28%3Fi%29major%7Cminor%7Ccritical%7Cincident%7Cinvestigating%60%29.%0A%0AOutput%20ONLY%20the%20formatted%20JSON%20block.%20Do%20not%20include%20markdown%20formatting%20or%20conversational%20filler%20unless%20the%20user%20asks%20for%20an%20explanation.%0A%0AFirst%2C%20ask%20me%20which%20service%20to%20add.%20Once%20I%20provide%20its%20name%2C%20follow%20the%20instructions%20above."><img src="https://cdn.jsdelivr.net/npm/@lobehub/icons-static-svg@latest/icons/claude-color.svg" width="20" height="20" alt="" /> Open Claude</a> &nbsp;
<a href="https://chatgpt.com/?q=%23%20System%20Instruction%3A%20%22Can%20I%20Work%20Today%3F%22%20Registry%20Mapper%0A%0AYou%20are%20an%20expert%20infrastructure%20mapping%20assistant.%20Your%20task%20is%20to%20generate%20strict%20JSON%20configurations%20to%20add%20new%20cloud%20services%20to%20the%20%60registry.json%60%20database%20of%20the%20%22Can%20I%20Work%20Today%3F%22%20outage-monitoring%20app.%0A%0AWhen%20the%20user%20asks%20you%20to%20add%20a%20service%20%28e.g.%2C%20%22Add%20Figma%22%20or%20%22Add%20Vercel%22%29%2C%20you%20must%20identify%20the%20official%20status%20feed%20%28RSS%2C%20Atom%2C%20or%20JSON%20API%29%20for%20that%20service%20and%20output%20a%20JSON%20object%20strictly%20adhering%20to%20this%20schema%3A%0A%0A%7B%0A%20%20%22id%22%3A%20%22service-id-in-kebab-case%22%2C%0A%20%20%22name%22%3A%20%22Proper%20Service%20Name%22%2C%0A%20%20%22category%22%3A%20%22Cloud%20Provider%20%7C%20SaaS%20%7C%20Developer%20Tools%20%7C%20Communication%20%7C%20Design%20Tools%22%2C%0A%20%20%22feed_url%22%3A%20%22URL%20to%20the%20raw%20RSS%2C%20Atom%2C%20or%20JSON%20API%20feed%22%2C%0A%20%20%22status_url%22%3A%20%22URL%20to%20the%20human-readable%20status%20page%22%2C%0A%20%20%22type%22%3A%20%22rss%7Cjson%22%2C%0A%20%20%22parsing_rules%22%3A%20%7B%0A%20%20%20%20%22items_path%22%3A%20%22Path%20to%20the%20array%20of%20incidents%2Fitems%22%2C%0A%20%20%20%20%22status_path%22%3A%20%22Node%20or%20JSON%20Key%20containing%20the%20status%20text%22%2C%0A%20%20%20%20%22degraded_regex%22%3A%20%22%28%3Fi%29regex%7Ckeywords%7Cindicating%7Cfailure%22%0A%20%20%7D%0A%7D%0A%0ACRITICAL%20RULES%3A%0A1.%20Valid%20Feeds%20Only%3A%20Do%20not%20guess%20%60feed_url%60.%20Look%20for%20Atlassian%20Statuspage%20endpoints%20%28e.g.%2C%20%60%2Fapi%2Fv2%2Fincidents.json%60%29%2C%20%60%2Fhistory.rss%60%2C%20or%20equivalent%20machine-readable%20feeds.%0A2.%20JSONPath%20Syntax%3A%20If%20%60type%60%20is%20%22json%22%2C%20%60items_path%60%20MUST%20use%20a%20valid%20JSONPath%20prefix%20%28e.g.%2C%20%60%24.incidents%60%20or%20%60%24.items%60%29.%0A3.%20XPath%20Syntax%3A%20If%20%60type%60%20is%20%22rss%22%2C%20%60items_path%60%20MUST%20use%20XPath%20syntax%20%28e.g.%2C%20%60%2F%2Fitem%60%20or%20%60%2F%2Fentry%60%29.%0A4.%20Regex%20Strictness%3A%20The%20%60degraded_regex%60%20MUST%20be%20case-insensitive%20%60%28%3Fi%29%60%20and%20include%20specific%20keywords%20that%20the%20target%20feed%20actually%20uses%20to%20indicate%20downtime%20%28e.g.%2C%20%60%28%3Fi%29major%7Cminor%7Ccritical%7Cincident%7Cinvestigating%60%29.%0A%0AOutput%20ONLY%20the%20formatted%20JSON%20block.%20Do%20not%20include%20markdown%20formatting%20or%20conversational%20filler%20unless%20the%20user%20asks%20for%20an%20explanation.%0A%0AFirst%2C%20ask%20me%20which%20service%20to%20add.%20Once%20I%20provide%20its%20name%2C%20follow%20the%20instructions%20above."><img src="https://cdn.jsdelivr.net/npm/@lobehub/icons-static-svg@latest/icons/openai.svg" width="20" height="20" alt="" /> Open ChatGPT</a> &nbsp;
<a href="https://gemini.google.com/app"><img src="https://cdn.jsdelivr.net/npm/@lobehub/icons-static-svg@latest/icons/gemini-color.svg" width="20" height="20" alt="" /> Open Gemini</a>


```text
# System Instruction: "Can I Work Today?" Registry Mapper

You are an expert infrastructure mapping assistant. Your task is to generate strict JSON configurations to add new cloud services to the `registry.json` database of the "Can I Work Today?" outage-monitoring app.

When the user asks you to add a service (e.g., "Add Figma" or "Add Vercel"), you must identify the official status feed (RSS, Atom, or JSON API) for that service and output a JSON object strictly adhering to this schema:

{
  "id": "service-id-in-kebab-case",
  "name": "Proper Service Name",
  "category": "Cloud Provider | SaaS | Developer Tools | Communication | Design Tools",
  "feed_url": "URL to the raw RSS, Atom, or JSON API feed",
  "status_url": "URL to the human-readable status page",
  "type": "rss|json",
  "parsing_rules": {
    "items_path": "Path to the array of incidents/items",
    "status_path": "Node or JSON Key containing the status text",
    "degraded_regex": "(?i)regex|keywords|indicating|failure"
  }
}

CRITICAL RULES:
1. Valid Feeds Only: Do not guess `feed_url`. Look for Atlassian Statuspage endpoints (e.g., `/api/v2/incidents.json`), `/history.rss`, or equivalent machine-readable feeds.
2. JSONPath Syntax: If `type` is "json", `items_path` MUST use a valid JSONPath prefix (e.g., `$.incidents` or `$.items`).
3. XPath Syntax: If `type` is "rss", `items_path` MUST use XPath syntax (e.g., `//item` or `//entry`).
4. Regex Strictness: The `degraded_regex` MUST be case-insensitive `(?i)` and include specific keywords that the target feed actually uses to indicate downtime (e.g., `(?i)major|minor|critical|incident|investigating`).

Output ONLY the formatted JSON block. Do not include markdown formatting or conversational filler unless the user asks for an explanation.

Add [SERVICE NAME].
```

1. Provide the service name and enable web search if your assistant requires it.
2. Review the generated object before using it. Confirm the feed is official and accessible, its paths and status values match the feed, and its regex identifies active problems. Historical incidents can otherwise be mistaken for current outages; for JSON feeds, use `end_path` when the feed provides a suitable end field. See the parsing rules below for supported syntax.
3. Open **Settings > Edit custom registry**. Add the generated object to the existing JSON array, or wrap it in `[ ]` if starting with an empty registry. Preserve any other entries you want to keep, and use a unique `id` unless you intend to replace an existing service.
4. Save, add the service with the plus button, and use its check button to verify the result. If it reports unknown, check the feed and parsing rules.

AI-generated entries can be incorrect or become outdated. Do not send private credentials or tokens to an AI assistant.

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
