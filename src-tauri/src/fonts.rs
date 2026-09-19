//! Font families as DirectWrite sees them. The IME engine (`ui.dll`,
//! `FontWin::GetDwriteTextFmt`) and the WebView preview both resolve
//! `facename` through DirectWrite, so GDI-era names such as
//! "Microsoft YaHei UI Light" or "Noto Sans SC Black" silently fall back to
//! the default font. Listing DirectWrite families keeps preview and IME honest.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteLocalizedStrings,
    DWRITE_FACTORY_TYPE_SHARED,
};

pub const DEFAULT_FACE: &str = "Microsoft YaHei";

#[derive(Clone, Debug, Serialize)]
pub struct FontChoice {
    pub face: String,
}

#[derive(Deserialize)]
struct FontChoiceRaw {
    face: Option<String>,
    zh: Option<String>,
}

fn default_face() -> String {
    DEFAULT_FACE.into()
}

impl Default for FontChoice {
    fn default() -> Self {
        Self {
            face: default_face(),
        }
    }
}

impl<'de> Deserialize<'de> for FontChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = FontChoiceRaw::deserialize(deserializer)?;
        let face = raw
            .face
            .or(raw.zh)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_face);
        Ok(Self { face })
    }
}

impl FontChoice {
    pub fn normalize(self) -> Result<Self, String> {
        Ok(Self {
            face: sanitize_face(&self.face)?,
        })
    }
}

#[derive(Clone, Serialize)]
pub struct FontItem {
    /// Name written to `facename`; zh-CN localized name when the font has one.
    pub name: String,
    /// Other localized names of the same family (e.g. the English one).
    pub aliases: Vec<String>,
    pub keys: Vec<String>,
}

#[derive(Serialize)]
pub struct FontLists {
    pub faces: Vec<FontItem>,
}

pub fn sanitize_face(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("字体名不能为空".into());
    }
    if s.chars()
        .any(|c| c.is_control() || matches!(c, '"' | '<' | '>' | '&'))
    {
        return Err("字体名含非法字符".into());
    }
    if s.chars().count() > 63 {
        return Err("字体名过长".into());
    }
    Ok(s.to_string())
}

pub fn xml_face(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

struct Family {
    /// (locale, name) pairs straight from the font's name table.
    names: Vec<(String, String)>,
    symbol: bool,
    cjk: bool,
}

fn utf16_str(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn localized_names(strings: &IDWriteLocalizedStrings) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let count = unsafe { strings.GetCount() };
    for i in 0..count {
        let Ok(name_len) = (unsafe { strings.GetStringLength(i) }) else {
            continue;
        };
        let mut name = vec![0u16; name_len as usize + 1];
        if unsafe { strings.GetString(i, &mut name) }.is_err() {
            continue;
        }
        let loc_len = unsafe { strings.GetLocaleNameLength(i) }.unwrap_or(0);
        let mut locale = vec![0u16; loc_len as usize + 1];
        let _ = unsafe { strings.GetLocaleName(i, &mut locale) };
        let name = utf16_str(&name).trim().to_string();
        if !name.is_empty() {
            out.push((utf16_str(&locale).to_lowercase(), name));
        }
    }
    out
}

fn enumerate_families() -> Vec<Family> {
    let mut out = Vec::new();
    let Ok(factory) =
        (unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) })
    else {
        return out;
    };
    let mut collection: Option<IDWriteFontCollection> = None;
    if unsafe { factory.GetSystemFontCollection(&mut collection, false) }.is_err() {
        return out;
    }
    let Some(collection) = collection else {
        return out;
    };
    let count = unsafe { collection.GetFontFamilyCount() };
    for i in 0..count {
        let Ok(family) = (unsafe { collection.GetFontFamily(i) }) else {
            continue;
        };
        let Ok(strings) = (unsafe { family.GetFamilyNames() }) else {
            continue;
        };
        let names = localized_names(&strings);
        if names.is_empty() {
            continue;
        }
        let (symbol, cjk) = match unsafe { family.GetFont(0) } {
            Ok(font) => (
                unsafe { font.IsSymbolFont() }.as_bool(),
                unsafe { font.HasCharacter('中' as u32) }
                    .map(|b| b.as_bool())
                    .unwrap_or(false),
            ),
            Err(_) => (false, false),
        };
        out.push(Family { names, symbol, cjk });
    }
    out
}

/// Prefer the Simplified Chinese name, then any Chinese, then English.
fn display_name(names: &[(String, String)]) -> String {
    let pick = |pred: &dyn Fn(&str) -> bool| {
        names
            .iter()
            .find(|(loc, _)| pred(loc))
            .map(|(_, n)| n.clone())
    };
    pick(&|l| l == "zh-cn" || l.starts_with("zh-hans") || l == "zh")
        .or_else(|| pick(&|l| l.starts_with("zh")))
        .or_else(|| pick(&|l| l.starts_with("en")))
        .or_else(|| names.first().map(|(_, n)| n.clone()))
        .unwrap_or_default()
}

fn search_keys(name: &str) -> Vec<String> {
    use pinyin::ToPinyin;
    let mut full = String::new();
    let mut initials = String::new();
    let mut has_py = false;
    for ch in name.chars() {
        if let Some(py) = ch.to_pinyin() {
            has_py = true;
            let plain = py.plain();
            full.push_str(plain);
            if let Some(c) = plain.chars().next() {
                initials.push(c);
            }
        }
    }
    let mut keys = vec![name.to_lowercase().replace(' ', "")];
    if has_py {
        keys.push(full);
        if !initials.is_empty() {
            keys.push(initials);
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn build_item(names: &[(String, String)]) -> FontItem {
    let name = display_name(names);
    let mut aliases: Vec<String> = names
        .iter()
        .map(|(_, n)| n.clone())
        .filter(|n| n != &name)
        .collect();
    aliases.sort();
    aliases.dedup();
    let mut keys = search_keys(&name);
    for alias in &aliases {
        keys.extend(search_keys(alias));
    }
    keys.sort();
    keys.dedup();
    FontItem {
        name,
        aliases,
        keys,
    }
}

pub fn list_installed() -> FontLists {
    let mut seen = HashSet::new();
    let mut items: Vec<(bool, FontItem)> = Vec::new();
    for family in enumerate_families() {
        if family.symbol {
            continue;
        }
        let item = build_item(&family.names);
        if item.name.is_empty() || !seen.insert(item.name.to_lowercase()) {
            continue;
        }
        items.push((family.cjk, item));
    }
    let has_default = items
        .iter()
        .any(|(_, it)| it.name == DEFAULT_FACE || it.aliases.iter().any(|a| a == DEFAULT_FACE));
    if !has_default {
        items.push((true, build_item(&[("en-us".into(), DEFAULT_FACE.into())])));
    }
    // Fonts that can actually draw Chinese first, then everything else.
    items.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    FontLists {
        faces: items.into_iter().map(|(_, it)| it).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_markup() {
        assert!(sanitize_face("").is_err());
        assert!(sanitize_face("A\"B").is_err());
        assert_eq!(sanitize_face("  KaiTi  ").unwrap(), "KaiTi");
    }

    #[test]
    fn lists_directwrite_families_with_yahei() {
        let lists = list_installed();
        assert!(lists.faces.len() > 3);
        let yahei = lists
            .faces
            .iter()
            .find(|f| f.name == DEFAULT_FACE || f.aliases.iter().any(|a| a == DEFAULT_FACE))
            .expect("Microsoft YaHei family");
        assert!(yahei.keys.iter().any(|k| k == "microsoftyahei"));
        // GDI-only weight names must not leak in: the engine cannot resolve them.
        assert!(!lists
            .faces
            .iter()
            .any(|f| f.name.ends_with(" Light") && f.name.starts_with("Microsoft YaHei")));
    }

    #[test]
    fn display_prefers_simplified_chinese_then_english() {
        let names = vec![
            ("en-us".to_string(), "Microsoft YaHei".to_string()),
            ("zh-cn".to_string(), "微软雅黑".to_string()),
        ];
        let item = build_item(&names);
        assert_eq!(item.name, "微软雅黑");
        assert_eq!(item.aliases, vec!["Microsoft YaHei".to_string()]);
        assert!(item.keys.iter().any(|k| k == "microsoftyahei"));
        assert!(item.keys.iter().any(|k| k == "weiruanyahei"));
        assert!(item.keys.iter().any(|k| k == "wryh"));
        let english_only = vec![("en-us".to_string(), "Noto Sans SC".to_string())];
        assert_eq!(build_item(&english_only).name, "Noto Sans SC");
    }

    #[test]
    fn reads_legacy_zh_field() {
        let choice: FontChoice =
            serde_json::from_str(r#"{"zh":"华文宋体","en":"Adobe 黑体 Std R"}"#).unwrap();
        assert_eq!(choice.face, "华文宋体");
    }

    #[test]
    fn pinyin_keys_match_youyuan() {
        let keys = search_keys("幼圆");
        assert!(keys.iter().any(|k| k == "youyuan"));
        assert!(keys.iter().any(|k| k.starts_with("youy")));
    }
}
