use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;
use std::collections::HashSet;

use crate::db::backend::BackendConnection;
use crate::db::daily_notes;

fn validate_section(section: &str) -> Result<()> {
    match section {
        "market" | "decisions" | "system" | "analysis" | "events" | "general" | "alert" => Ok(()),
        _ => bail!(
            "invalid section '{}'. Valid: market, decisions, system, analysis, events, general, alert",
            section
        ),
    }
}

// ---------------------------------------------------------------------------
// Novelty scoring (character-trigram Jaccard similarity)
//
// Similarity metric: Jaccard over character-trigram SETS of the normalized
// text (lowercase, punctuation stripped, whitespace runs collapsed). Chosen
// over cosine because the sets are unweighted (a note that repeats a phrase
// many times should not look more similar than one that states it once) and
// Jaccard is the standard set-overlap measure for that case.
//
// novelty_score = 1 − max(similarity vs the same author's last 20 notes).
// ---------------------------------------------------------------------------

/// How many of the author's most recent notes to compare against.
const NOVELTY_LOOKBACK_NOTES: usize = 20;
/// Below this novelty (≥85% similar to an existing note) the writer is
/// warned that the note is repetitive.
const NOVELTY_REPETITIVE_THRESHOLD: f64 = 0.15;
/// Mutual-similarity threshold for `notes repetition` clustering.
const REPETITION_SIMILARITY_THRESHOLD: f64 = 0.85;
/// Safety cap on notes considered by `notes repetition` (the window query
/// already bounds this; the cap guards pathological note volumes).
const REPETITION_MAX_NOTES: usize = 500;

/// Lowercase, replace punctuation with spaces, collapse whitespace runs.
fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true;
    for c in text.chars() {
        let c = if c.is_alphanumeric() {
            Some(c.to_ascii_lowercase())
        } else {
            None
        };
        match c {
            Some(c) => {
                out.push(c);
                last_was_space = false;
            }
            None => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
        }
    }
    out.trim_end().to_string()
}

/// Character-trigram set of the normalized text.
fn trigram_set(text: &str) -> HashSet<String> {
    let normalized = normalize_text(text);
    let chars: Vec<char> = normalized.chars().collect();
    let mut set = HashSet::new();
    if chars.len() < 3 {
        if !normalized.is_empty() {
            set.insert(normalized);
        }
        return set;
    }
    for window in chars.windows(3) {
        set.insert(window.iter().collect());
    }
    set
}

/// Jaccard similarity over character-trigram sets. 0.0 = disjoint, 1.0 = identical.
fn trigram_jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let intersection = small.iter().filter(|t| large.contains(*t)).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

/// Similarity between two raw texts (test/diagnostic convenience wrapper).
#[cfg(test)]
fn text_similarity(a: &str, b: &str) -> f64 {
    trigram_jaccard(&trigram_set(a), &trigram_set(b))
}

/// (novelty_score, most_similar prior note) for `content` vs the same
/// author's last `NOVELTY_LOOKBACK_NOTES` notes (any section).
fn compute_novelty(
    backend: &BackendConnection,
    author: &str,
    content: &str,
) -> Result<(f64, Option<daily_notes::DailyNote>)> {
    let recent = daily_notes::list_notes_backend(
        backend,
        None,
        None,
        Some(NOVELTY_LOOKBACK_NOTES),
        Some(author),
    )?;
    let new_set = trigram_set(content);
    let mut max_similarity = 0.0f64;
    let mut most_similar: Option<daily_notes::DailyNote> = None;
    for note in recent {
        let sim = trigram_jaccard(&new_set, &trigram_set(&note.content));
        if sim > max_similarity {
            max_similarity = sim;
            most_similar = Some(note);
        }
    }
    Ok(((1.0 - max_similarity).clamp(0.0, 1.0), most_similar))
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    date: Option<&str>,
    section: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
    author: Option<&str>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let content = value.ok_or_else(|| anyhow::anyhow!("note content required"))?;
            let note_date = date
                .map(|d| d.to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
            let sec = section.unwrap_or("general");
            validate_section(sec)?;
            let author_value = author.unwrap_or("system");

            // Novelty vs the same author's last 20 notes (any section).
            // Never blocks the write — repetition is a notice, not an error.
            let (novelty_score, most_similar) = compute_novelty(backend, author_value, content)?;
            let most_similar_note_id = most_similar.as_ref().map(|n| n.id);

            let new_id = daily_notes::add_note_with_novelty_backend(
                backend,
                &note_date,
                sec,
                content,
                author_value,
                Some(novelty_score),
            )?;

            let repetitive = novelty_score < NOVELTY_REPETITIVE_THRESHOLD;
            if json_output {
                let rows = daily_notes::list_notes_backend(backend, None, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    let mut payload = serde_json::to_value(&row)?;
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert("novelty_score".into(), json!(novelty_score));
                        obj.insert("most_similar_note_id".into(), json!(most_similar_note_id));
                        obj.insert("repetitive".into(), json!(repetitive));
                    }
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
            } else {
                println!("Added note #{} ({}/{})", new_id, note_date, sec);
                if repetitive {
                    if let Some(similar) = most_similar.as_ref() {
                        println!(
                            "⚠ repetitive: {:.0}% similar to note #{} ({}) — consider updating the thesis table instead of re-deriving",
                            (1.0 - novelty_score) * 100.0,
                            similar.id,
                            similar.date
                        );
                    }
                }
            }
        }

        "list" => {
            if let Some(s) = section {
                validate_section(s)?;
            }
            let rows = daily_notes::list_notes_backend(backend, date, section, limit, author)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "notes": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No notes found.");
            } else {
                println!("Daily notes ({}):", rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}:{}] {}",
                        row.id, row.date, row.section, row.content
                    );
                }
            }
        }

        "search" => {
            let query = value.ok_or_else(|| anyhow::anyhow!("search query required"))?;
            let rows = daily_notes::search_notes_backend(backend, query, since, limit)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "notes": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No notes matched '{}'.", query);
            } else {
                println!("Search results for '{}' ({}):", query, rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}:{}] {}",
                        row.id, row.date, row.section, row.content
                    );
                }
            }
        }

        "remove" => {
            let note_id = id.ok_or_else(|| anyhow::anyhow!("--id required for remove"))?;
            daily_notes::remove_note_backend(backend, note_id)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "removed": note_id }))?
                );
            } else {
                println!("Removed note #{}", note_id);
            }
        }

        _ => bail!(
            "unknown notes action '{}'. Valid: add, list, search, remove",
            action
        ),
    }

    Ok(())
}

/// One greedily-built cluster of mutually similar notes by one author.
struct RepetitionCluster {
    author: String,
    note_ids: Vec<i64>,
    first_date: String,
    last_date: String,
    excerpt: String,
}

/// Greedy clustering per author: a note joins the first cluster (same
/// author) whose representative — the cluster's earliest note — it is
/// ≥ `REPETITION_SIMILARITY_THRESHOLD` similar to. Input must be in
/// chronological order.
fn cluster_notes(notes: &[daily_notes::DailyNote]) -> Vec<RepetitionCluster> {
    struct Working {
        cluster: RepetitionCluster,
        representative: HashSet<String>,
    }
    let mut clusters: Vec<Working> = Vec::new();
    for note in notes {
        let set = trigram_set(&note.content);
        let mut placed = false;
        for w in clusters.iter_mut() {
            if w.cluster.author == note.author
                && trigram_jaccard(&w.representative, &set) >= REPETITION_SIMILARITY_THRESHOLD
            {
                w.cluster.note_ids.push(note.id);
                w.cluster.last_date = note.date.clone();
                placed = true;
                break;
            }
        }
        if !placed {
            let excerpt: String = note.content.chars().take(100).collect();
            clusters.push(Working {
                cluster: RepetitionCluster {
                    author: note.author.clone(),
                    note_ids: vec![note.id],
                    first_date: note.date.clone(),
                    last_date: note.date.clone(),
                    excerpt,
                },
                representative: set,
            });
        }
    }
    clusters.into_iter().map(|w| w.cluster).collect()
}

/// `pftui journal notes repetition` — cluster an author's recent notes by
/// mutual trigram-Jaccard similarity ≥ 0.85 and surface the top repeated
/// clusters ("you have written this note 9 times").
pub fn run_repetition(
    backend: &BackendConnection,
    author: Option<&str>,
    days: i64,
    json_output: bool,
) -> Result<()> {
    let days = days.max(1);
    let cutoff = (Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();

    let mut notes = daily_notes::list_notes_backend(backend, None, None, None, author)?;
    notes.retain(|n| n.date.as_str() >= cutoff.as_str());
    notes.truncate(REPETITION_MAX_NOTES);
    // Chronological order so cluster first/last dates read naturally.
    notes.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));

    let clusters = cluster_notes(&notes);
    let mut repeated: Vec<&RepetitionCluster> =
        clusters.iter().filter(|c| c.note_ids.len() >= 2).collect();
    repeated.sort_by_key(|c| std::cmp::Reverse(c.note_ids.len()));

    if json_output {
        let payload: Vec<serde_json::Value> = repeated
            .iter()
            .map(|c| {
                json!({
                    "author": c.author,
                    "count": c.note_ids.len(),
                    "first_date": c.first_date,
                    "last_date": c.last_date,
                    "excerpt": c.excerpt,
                    "note_ids": c.note_ids,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "clusters": payload,
                "cluster_count": repeated.len(),
                "window_days": days,
                "similarity_threshold": REPETITION_SIMILARITY_THRESHOLD,
                "notes_considered": notes.len(),
            }))?
        );
    } else if repeated.is_empty() {
        println!(
            "No repeated note clusters in the last {} day(s) ({} notes checked, threshold ≥{:.0}% similar).",
            days,
            notes.len(),
            REPETITION_SIMILARITY_THRESHOLD * 100.0
        );
    } else {
        println!(
            "Repeated note clusters (last {} days, ≥{:.0}% mutual similarity):",
            days,
            REPETITION_SIMILARITY_THRESHOLD * 100.0
        );
        for cluster in &repeated {
            println!(
                "  ×{} [{}] {} → {}  \"{}\"",
                cluster.note_ids.len(),
                cluster.author,
                cluster.first_date,
                cluster.last_date,
                cluster.excerpt
            );
        }
        println!(
            "\n{} cluster(s). Consider consolidating each into the thesis table instead of re-deriving.",
            repeated.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_section_accepts_all_valid() {
        for section in &[
            "market",
            "decisions",
            "system",
            "analysis",
            "events",
            "general",
            "alert",
        ] {
            assert!(
                validate_section(section).is_ok(),
                "section '{}' should be valid",
                section
            );
        }
    }

    #[test]
    fn test_validate_section_rejects_invalid() {
        for section in &["alerts", "foo", "trading", ""] {
            assert!(
                validate_section(section).is_err(),
                "section '{}' should be invalid",
                section
            );
        }
    }

    fn setup_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn notes_add_persists_author_flag() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("LOW: pre-market scan"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-low"),
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].author, "analyst-low");
        assert_eq!(rows[0].content, "LOW: pre-market scan");
    }

    #[test]
    fn notes_list_filters_by_author() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("low note"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-low"),
            false,
        )
        .unwrap();
        run(
            &backend,
            "add",
            Some("medium note"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-medium"),
            false,
        )
        .unwrap();
        let lows = daily_notes::list_notes_backend(&backend, None, None, None, Some("analyst-low"))
            .unwrap();
        assert_eq!(lows.len(), 1);
        assert_eq!(lows[0].content, "low note");
    }

    #[test]
    fn similarity_identical_and_disjoint() {
        assert!(text_similarity("BTC holding above the 200d MA", "BTC holding above the 200d MA") > 0.999);
        assert!(text_similarity("gold breaks resistance", "uranium spot market tightness") < 0.2);
        // Normalization: case + punctuation differences are ignored.
        assert!(text_similarity("BTC: holding, above 200d!", "btc holding above 200d") > 0.999);
    }

    #[test]
    fn similarity_near_duplicate_scores_high() {
        let a = "MEDIUM: gold consolidating above 4000, central bank bid intact, ETF flows positive";
        let b = "MEDIUM: gold consolidating above 4100, central bank bid intact, ETF flows positive";
        assert!(text_similarity(a, b) > 0.85, "got {}", text_similarity(a, b));
    }

    #[test]
    fn notes_add_stores_novelty_score() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("gold consolidating above four thousand with central bank bid intact"),
            None,
            Some("2026-06-01"),
            Some("analysis"),
            None,
            None,
            Some("analyst-medium"),
            false,
        )
        .unwrap();
        // First note from an author has nothing to repeat — full novelty.
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        assert_eq!(rows[0].novelty_score, Some(1.0));

        // A near-duplicate from the same author scores near zero novelty.
        run(
            &backend,
            "add",
            Some("gold consolidating above four thousand with central bank bid intact!"),
            None,
            Some("2026-06-02"),
            Some("analysis"),
            None,
            None,
            Some("analyst-medium"),
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        let dup = rows.iter().find(|r| r.date == "2026-06-02").unwrap();
        assert!(dup.novelty_score.unwrap() < NOVELTY_REPETITIVE_THRESHOLD);

        // The same text from a DIFFERENT author compares only within that
        // author's history — full novelty again.
        run(
            &backend,
            "add",
            Some("gold consolidating above four thousand with central bank bid intact"),
            None,
            Some("2026-06-02"),
            Some("analysis"),
            None,
            None,
            Some("analyst-high"),
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(
            &backend,
            None,
            None,
            None,
            Some("analyst-high"),
        )
        .unwrap();
        assert_eq!(rows[0].novelty_score, Some(1.0));
    }

    #[test]
    fn repetition_clusters_repeated_notes() {
        let backend = setup_backend();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        for i in 0..3 {
            run(
                &backend,
                "add",
                Some(&format!(
                    "silver supply deficit thesis intact, COMEX registered drawdown continuing day {}",
                    i
                )),
                None,
                Some(&today),
                Some("analysis"),
                None,
                None,
                Some("analyst-medium"),
                false,
            )
            .unwrap();
        }
        run(
            &backend,
            "add",
            Some("uranium term price holding while spot drifts"),
            None,
            Some(&today),
            Some("analysis"),
            None,
            None,
            Some("analyst-medium"),
            false,
        )
        .unwrap();
        // The three near-duplicates form one cluster; the uranium note is alone.
        let mut notes =
            daily_notes::list_notes_backend(&backend, None, None, None, Some("analyst-medium"))
                .unwrap();
        notes.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
        let clusters = cluster_notes(&notes);
        assert_eq!(clusters.len(), 2);
        let repeated: Vec<_> = clusters.iter().filter(|c| c.note_ids.len() >= 2).collect();
        assert_eq!(repeated.len(), 1);
        assert_eq!(repeated[0].note_ids.len(), 3);
        assert!(repeated[0].excerpt.contains("silver supply deficit"));
        assert_eq!(repeated[0].author, "analyst-medium");

        // Smoke: the command runs and does not error in both output modes.
        run_repetition(&backend, Some("analyst-medium"), 30, false).unwrap();
        run_repetition(&backend, Some("analyst-medium"), 30, true).unwrap();
        run_repetition(&backend, None, 30, true).unwrap();
    }

    #[test]
    fn cluster_notes_separates_authors() {
        let mk = |id: i64, author: &str, content: &str| daily_notes::DailyNote {
            id,
            date: "2026-06-01".into(),
            section: "analysis".into(),
            content: content.into(),
            author: author.into(),
            created_at: "2026-06-01 00:00:00".into(),
            novelty_score: None,
        };
        let same = "BTC holding the range, no change to the cycle thesis today";
        let notes = vec![mk(1, "analyst-low", same), mk(2, "analyst-high", same)];
        // Identical text but different authors → never merged.
        let clusters = cluster_notes(&notes);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn trigram_set_handles_short_and_empty() {
        assert!(trigram_set("").is_empty());
        assert_eq!(trigram_set("ab").len(), 1);
        assert!(text_similarity("", "") > 0.999);
        assert!(text_similarity("", "anything at all") < 0.001);
    }

    #[test]
    fn notes_add_defaults_author_to_system() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("no author"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        assert_eq!(rows[0].author, "system");
    }
}
