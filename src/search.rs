//! Online search for the registries that have no local catalogue: npm and Bun (both use
//! the npm registry) and Cargo (crates.io). Results are not installed. Each search asks
//! the registry once and gives up after 25 seconds, so an offline machine just gets an
//! error for that registry.

use crate::track::OutputTracked;
use std::process::Command;

use serde_json::Value;

use crate::model::{Package, Source};

/// Search one registry for `query`. Returns an error message when the registry cannot be
/// reached or the query is not allowed.
pub fn search(src: Source, query: &str) -> Result<Vec<Package>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // A query that starts with '-' would be read as an option by the tool.
    if query.starts_with('-') {
        return Err("search terms cannot start with '-'".into());
    }
    match src {
        Source::Npm | Source::Bun => npm_search(src, query),
        Source::Cargo => cargo_search(query),
        _ => Ok(Vec::new()),
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("timeout")
        .arg("25")
        .arg(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("NO_UPDATE_NOTIFIER", "1")
        .output_tracked()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if out.status.code() == Some(124) {
        return Err("timed out (is the network reachable?)".into());
    }
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("search failed");
        return Err(line.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn npm_search(src: Source, query: &str) -> Result<Vec<Package>, String> {
    let text = run("npm", &["search", "--json", "--searchlimit=40", query])?;
    parse_npm(src, &text)
}

/// Parse `npm search --json` output: an array of objects with name, version and description.
pub fn parse_npm(src: Source, text: &str) -> Result<Vec<Package>, String> {
    let items: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("bad npm output: {e}"))?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?;
            let mut p = Package::new(src, name);
            p.candidate = item.get("version").and_then(Value::as_str).unwrap_or("").to_string();
            p.description = item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            p.origin = "npm registry".to_string();
            p.online = true;
            Some(p)
        })
        .collect())
}

/// `cargo search` prints lines like `ripgrep = "15.2.0"    # description`.
fn cargo_search(query: &str) -> Result<Vec<Package>, String> {
    let text = run("cargo", &["search", "--limit", "40", query])?;
    Ok(parse_cargo(&text))
}

pub fn parse_cargo(text: &str) -> Vec<Package> {
    text
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(" = \"")?;
            let (version, rest) = rest.split_once('"')?;
            let description = rest.split_once('#').map(|(_, d)| d.trim()).unwrap_or("");
            let mut p = Package::new(Source::Cargo, name.trim());
            p.candidate = version.to_string();
            p.description = description.to_string();
            p.origin = "crates.io".to_string();
            p.online = true;
            Some(p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_search_lines_and_skips_the_trailer() {
        let text = "ripgrep = \"15.2.0\"       # line-oriented search tool\n\
                    gist-search = \"1.3.2\"    # Indexed code search\n\
                    ... and 803 crates more (use --limit N to see more)\n";
        let found = parse_cargo(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "ripgrep");
        assert_eq!(found[0].candidate, "15.2.0");
        assert_eq!(found[0].description, "line-oriented search tool");
        assert_eq!(found[1].origin, "crates.io");
        assert!(found.iter().all(|p| p.online && !p.is_installed()));
    }

    #[test]
    fn parses_npm_search_json() {
        let text = r#"[{"name":"ripgrep","version":"0.3.1","description":"Works with Node.js"},
                       {"name":"left-pad","version":"1.3.0"}]"#;
        let found = parse_npm(Source::Bun, text).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].source, Source::Bun);
        assert_eq!(found[0].candidate, "0.3.1");
        assert_eq!(found[1].description, "");
        assert!(found.iter().all(|p| p.online));
    }

    #[test]
    fn refuses_queries_that_look_like_options() {
        assert!(search(Source::Cargo, "--help").is_err());
        assert!(search(Source::Npm, "  ").unwrap().is_empty());
    }
}
