fn parse_faces(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in raw.lines() {
        let face = line.trim();
        if face.is_empty() || !seen.insert(face.to_string()) {
            continue;
        }
        out.push(face.to_string());
    }
    out
}

#[tauri::command]
pub fn get_kaomoji_groups() -> serde_json::Value {
    serde_json::json!({
        "casual": parse_faces(include_str!("../../kaomoji/Group_casual.txt")),
        "welcome": parse_faces(include_str!("../../kaomoji/Group_welcom.txt")),
        "great": parse_faces(include_str!("../../kaomoji/Group_great.txt")),
        "cancel": parse_faces(include_str!("../../kaomoji/Group_cancel.txt")),
        "bad": parse_faces(include_str!("../../kaomoji/Group_bad.txt")),
        "sleep": parse_faces(include_str!("../../kaomoji/Group_sleep.txt")),
        "wake": parse_faces(include_str!("../../kaomoji/Group_wake.txt")),
    })
}
