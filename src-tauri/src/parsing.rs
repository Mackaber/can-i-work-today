use super::{Health, ParsingRules};
use regex::Regex;
use roxmltree::{Document, Node};
use serde_json::Value;

pub(super) fn json_path_segments(path: &str, absolute: bool) -> Result<Vec<&str>, String> {
    let path = if path == "$" {
        return Ok(Vec::new());
    } else if let Some(path) = path.strip_prefix("$.") {
        path
    } else if !absolute && !path.starts_with('$') {
        path
    } else {
        return Err(format!("Invalid JSON path: {path}"));
    };
    let segments: Vec<_> = path.split('.').collect();
    if segments.iter().any(|segment| {
        segment.is_empty()
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }) {
        return Err(format!("Invalid JSON path: {path}"));
    }
    Ok(segments)
}

pub(super) fn xml_path_segments(path: &str, items: bool) -> Result<Vec<&str>, String> {
    let path = if items {
        path.strip_prefix("//")
            .ok_or_else(|| format!("XML items_path must start with //: {path}"))?
    } else {
        path
    };
    let segments: Vec<_> = path.split('/').collect();
    if segments.iter().any(|segment| {
        !segment
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    }) {
        return Err(format!("Invalid XML path: {path}"));
    }
    Ok(segments)
}

fn json_at_path<'a>(value: &'a Value, segments: &[&str]) -> Option<&'a Value> {
    segments
        .iter()
        .try_fold(value, |value, segment| value.get(*segment))
}

fn result_from_statuses<'a>(
    statuses: impl Iterator<Item = Option<&'a str>>,
    regex: &Regex,
) -> (Health, String) {
    let mut matched = Vec::new();
    let mut unknown = false;
    for status in statuses {
        match status.map(str::trim).filter(|status| !status.is_empty()) {
            Some(status) if regex.is_match(status) => {
                if matched.len() < 2 {
                    matched.push(status.chars().take(200).collect::<String>());
                }
            }
            Some(_) => {}
            None => unknown = true,
        }
    }
    if !matched.is_empty() {
        (Health::Degraded, matched.join("; "))
    } else if unknown {
        (
            Health::Unknown,
            "Incident status could not be interpreted".to_string(),
        )
    } else {
        (Health::Healthy, "No active incidents reported".to_string())
    }
}

pub(super) fn parse_json(body: &str, rules: &ParsingRules) -> Result<(Health, String), String> {
    rules.validate("json")?;
    let regex = Regex::new(&rules.degraded_regex).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let items = json_at_path(&value, &json_path_segments(&rules.items_path, true)?)
        .ok_or_else(|| format!("JSON items_path not found: {}", rules.items_path))?;
    let items: Vec<&Value> = match items {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![items],
        _ => {
            return Err(format!(
                "JSON items_path must select an array or object: {}",
                rules.items_path
            ))
        }
    };
    let status_path = json_path_segments(&rules.status_path, false)?;
    let end_path = rules
        .end_path
        .as_deref()
        .map(|path| json_path_segments(path, false))
        .transpose()?;
    let statuses = items.into_iter().filter_map(|item| {
        if let Some(path) = &end_path {
            match json_at_path(item, path) {
                Some(Value::Null) => {}
                Some(Value::String(end)) if end.is_empty() => {}
                Some(Value::String(_)) => return None,
                _ => return Some(None),
            }
        }
        Some(json_at_path(item, &status_path).and_then(Value::as_str))
    });
    Ok(result_from_statuses(statuses, &regex))
}

fn xml_child_path<'a, 'input>(
    mut node: Node<'a, 'input>,
    path: &[&str],
) -> Option<Node<'a, 'input>> {
    for segment in path {
        node = node.children().find(|child| child.has_tag_name(*segment))?;
    }
    Some(node)
}

pub(super) fn parse_rss(body: &str, rules: &ParsingRules) -> Result<(Health, String), String> {
    rules.validate("rss")?;
    let regex = Regex::new(&rules.degraded_regex).map_err(|error| error.to_string())?;
    let document = Document::parse(body).map_err(|error| error.to_string())?;
    if !document.root_element().has_tag_name("rss")
        || !document
            .root_element()
            .children()
            .any(|node| node.has_tag_name("channel"))
    {
        return Err("RSS channel is missing".to_string());
    }
    let items_path = xml_path_segments(&rules.items_path, true)?;
    let status_path = xml_path_segments(&rules.status_path, false)?;
    let mut items: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name(items_path[0]))
        .collect();
    for segment in &items_path[1..] {
        items = items
            .into_iter()
            .flat_map(|node| node.children().filter(|child| child.has_tag_name(*segment)))
            .collect();
    }
    let statuses: Vec<Option<String>> = items
        .into_iter()
        .map(|item| {
            xml_child_path(item, &status_path).map(|status| {
                status
                    .descendants()
                    .filter_map(|node| node.text().filter(|_| node.is_text()))
                    .collect()
            })
        })
        .collect();
    Ok(result_from_statuses(
        statuses.iter().map(|status| status.as_deref()),
        &regex,
    ))
}
