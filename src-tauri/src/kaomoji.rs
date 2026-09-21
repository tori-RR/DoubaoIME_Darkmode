use std::fs;
use std::path::PathBuf;

fn parse_faces(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in raw.lines() {
        let face = line.strip_prefix('\u{feff}').unwrap_or(line);
        if face.trim().is_empty() || !seen.insert(face.to_string()) {
            continue;
        }
        out.push(face.to_string());
    }
    out
}

fn external_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("kaomoji"));
        }
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kaomoji"));
    dirs
}

fn read_group(file: &str, embedded: &str) -> Vec<String> {
    for dir in external_dirs() {
        if let Ok(raw) = fs::read_to_string(dir.join(file)) {
            return parse_faces(&raw);
        }
    }
    parse_faces(embedded)
}

#[tauri::command]
pub fn get_kaomoji_groups() -> serde_json::Value {
    serde_json::json!({
        "casual": read_group("Group_casual.txt", include_str!("../../kaomoji/Group_casual.txt")),
        "welcome": read_group("Group_welcom.txt", include_str!("../../kaomoji/Group_welcom.txt")),
        "great": read_group("Group_great.txt", include_str!("../../kaomoji/Group_great.txt")),
        "cancel": read_group("Group_cancel.txt", include_str!("../../kaomoji/Group_cancel.txt")),
        "bad": read_group("Group_bad.txt", include_str!("../../kaomoji/Group_bad.txt")),
        "sleep": read_group("Group_sleep.txt", include_str!("../../kaomoji/Group_sleep.txt")),
        "wake": read_group("Group_wake.txt", include_str!("../../kaomoji/Group_wake.txt")),
    })
}
