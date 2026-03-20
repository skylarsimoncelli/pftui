use anyhow::{bail, Result};

use crate::config::{load_config, save_config, TrackedUniverse};

/// List all tracked universe groups and their symbols.
pub fn list(json: bool) -> Result<()> {
    let config = load_config()?;
    let universe = &config.tracked_universe;

    let all = universe.all_symbols();

    if json {
        let mut map = serde_json::Map::new();
        for name in TrackedUniverse::group_names() {
            if let Some(symbols) = universe.group(name) {
                map.insert(
                    name.to_string(),
                    serde_json::Value::Array(
                        symbols
                            .iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    ),
                );
            }
        }
        let mut root = serde_json::Map::new();
        root.insert("groups".to_string(), serde_json::Value::Object(map));
        root.insert(
            "total_symbols".to_string(),
            serde_json::Value::Number(serde_json::Number::from(all.len())),
        );
        root.insert(
            "unique_symbols".to_string(),
            serde_json::Value::Number(serde_json::Number::from(all.len())),
        );
        println!("{}", serde_json::to_string_pretty(&root)?);
    } else {
        println!("Tracked Universe\n");
        for name in TrackedUniverse::group_names() {
            if let Some(symbols) = universe.group(name) {
                if symbols.is_empty() {
                    println!("  {}: (empty)", name);
                } else {
                    println!("  {}: {}", name, symbols.join(", "));
                }
            }
        }
        println!("\n  Total: {} unique symbols", all.len());
    }
    Ok(())
}

/// Add a symbol to a universe group.
pub fn add(symbol: &str, group: &str, json: bool) -> Result<()> {
    let mut config = load_config()?;
    let upper = symbol.to_uppercase();

    let symbols = config
        .tracked_universe
        .group_mut(group)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown group '{}'. Valid groups: {}",
                group,
                TrackedUniverse::group_names().join(", ")
            )
        })?;

    if symbols.iter().any(|s| s.eq_ignore_ascii_case(&upper)) {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "already_exists",
                    "symbol": upper,
                    "group": group
                })
            );
        } else {
            println!("{} already in group '{}'", upper, group);
        }
        return Ok(());
    }

    symbols.push(upper.clone());
    save_config(&config)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "added",
                "symbol": upper,
                "group": group
            })
        );
    } else {
        println!("Added {} to group '{}'", upper, group);
    }
    Ok(())
}

/// Remove a symbol from a universe group.
pub fn remove(symbol: &str, group: &str, json: bool) -> Result<()> {
    let mut config = load_config()?;
    let upper = symbol.to_uppercase();

    let symbols = config
        .tracked_universe
        .group_mut(group)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown group '{}'. Valid groups: {}",
                group,
                TrackedUniverse::group_names().join(", ")
            )
        })?;

    let before = symbols.len();
    symbols.retain(|s| !s.eq_ignore_ascii_case(&upper));

    if symbols.len() == before {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "not_found",
                    "symbol": upper,
                    "group": group
                })
            );
        } else {
            bail!("{} not found in group '{}'", upper, group);
        }
        return Ok(());
    }

    save_config(&config)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "removed",
                "symbol": upper,
                "group": group
            })
        );
    } else {
        println!("Removed {} from group '{}'", upper, group);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::TrackedUniverse;

    #[test]
    fn default_universe_has_expected_groups() {
        let u = TrackedUniverse::default();
        assert!(!u.indices.is_empty());
        assert!(!u.sectors.is_empty());
        assert!(!u.commodities.is_empty());
        assert!(!u.fx.is_empty());
        assert!(!u.rates.is_empty());
        assert!(!u.crypto_majors.is_empty());
        assert!(u.custom.is_empty());
    }

    #[test]
    fn default_universe_contains_key_symbols() {
        let u = TrackedUniverse::default();
        assert!(u.indices.contains(&"SPY".to_string()));
        assert!(u.indices.contains(&"QQQ".to_string()));
        assert!(u.sectors.contains(&"XLK".to_string()));
        assert!(u.commodities.contains(&"GC=F".to_string()));
        assert!(u.fx.contains(&"DX-Y.NYB".to_string()));
        assert!(u.rates.contains(&"^TNX".to_string()));
        assert!(u.crypto_majors.contains(&"BTC-USD".to_string()));
    }

    #[test]
    fn all_symbols_deduplicates() {
        let mut u = TrackedUniverse::default();
        // Add a symbol that already exists in indices
        u.custom.push("SPY".to_string());
        let all = u.all_symbols();
        let spy_count = all.iter().filter(|s| *s == "SPY").count();
        assert_eq!(spy_count, 1);
    }

    #[test]
    fn group_names_are_complete() {
        let names = TrackedUniverse::group_names();
        assert_eq!(names.len(), 7);
        assert!(names.contains(&"indices"));
        assert!(names.contains(&"custom"));
    }

    #[test]
    fn group_accessor_returns_correct_data() {
        let u = TrackedUniverse::default();
        assert_eq!(u.group("indices"), Some(&u.indices));
        assert_eq!(u.group("custom"), Some(&u.custom));
        assert_eq!(u.group("nonexistent"), None);
    }

    #[test]
    fn group_mut_accessor_allows_mutation() {
        let mut u = TrackedUniverse::default();
        let custom = u.group_mut("custom").unwrap();
        custom.push("TEST".to_string());
        assert!(u.custom.contains(&"TEST".to_string()));
    }

    #[test]
    fn universe_roundtrip_toml() {
        let u = TrackedUniverse::default();
        let toml_str = toml::to_string_pretty(&u).unwrap();
        let parsed: TrackedUniverse = toml::from_str(&toml_str).unwrap();
        assert_eq!(u, parsed);
    }

    #[test]
    fn universe_deserialize_empty_uses_defaults() {
        let u: TrackedUniverse = toml::from_str("").unwrap();
        assert_eq!(u, TrackedUniverse::default());
    }

    #[test]
    fn universe_deserialize_partial_fills_defaults() {
        let toml_str = r#"custom = ["AAPL", "GOOG"]"#;
        let u: TrackedUniverse = toml::from_str(toml_str).unwrap();
        // Custom should be what we set
        assert_eq!(u.custom, vec!["AAPL".to_string(), "GOOG".to_string()]);
        // Others should be defaults
        assert_eq!(u.indices, TrackedUniverse::default().indices);
        assert_eq!(u.sectors, TrackedUniverse::default().sectors);
    }
}
