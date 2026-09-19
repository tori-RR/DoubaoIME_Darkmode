use crate::fonts::{xml_face, FontChoice};
#[cfg(test)]
use crate::skin;
use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::fs;
use std::path::Path;

fn default_emphasis() -> String {
    "#FFFFFF".into()
}

pub fn dark_colors() -> ThemeColors {
    ThemeColors {
        accent: "#8A8A8A".into(),
        background: "#2A2A2A".into(),
        foreground: "#F2F2F2".into(),
        emphasis: default_emphasis(),
    }
}

pub fn light_colors() -> ThemeColors {
    ThemeColors {
        accent: "#4F84FF".into(),
        background: "#FFFFFF".into(),
        foreground: "#000000".into(),
        emphasis: default_emphasis(),
    }
}

pub fn mix_colors() -> ThemeColors {
    ThemeColors {
        accent: "#7794E4".into(),
        background: "#3D57D6".into(),
        foreground: "#FEE5CA".into(),
        emphasis: default_emphasis(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    pub accent: String,
    pub background: String,
    pub foreground: String,
    #[serde(default = "default_emphasis")]
    pub emphasis: String,
}

impl ThemeColors {
    pub fn normalize(self) -> Result<Self, String> {
        Ok(Self {
            accent: normalize_hex(&self.accent)?,
            background: normalize_hex(&self.background)?,
            foreground: normalize_hex(&self.foreground)?,
            emphasis: normalize_hex(&self.emphasis)?,
        })
    }
}

pub fn normalize_hex(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("颜色须为 #RRGGBB：{raw}"));
    }
    Ok(format!("#{}", s.to_ascii_uppercase()))
}

fn hex_rgb(hex: &str) -> Result<(u8, u8, u8), String> {
    let h = normalize_hex(hex)?;
    let n = u32::from_str_radix(&h[1..], 16).map_err(|_| format!("坏颜色 {hex}"))?;
    Ok((
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ))
}

fn argb(hex: &str, alpha: u8) -> Result<String, String> {
    let (r, g, b) = hex_rgb(hex)?;
    Ok(format!("#{alpha:02X}{r:02X}{g:02X}{b:02X}"))
}

pub const GLASS_OPACITY_DEFAULT: u8 = 100;
pub const GLASS_OPACITY_MIN: u8 = 20;
pub const GLASS_OPACITY_MAX: u8 = 100;

pub fn clamp_glass_opacity(percent: u8) -> u8 {
    percent.clamp(GLASS_OPACITY_MIN, GLASS_OPACITY_MAX)
}

pub fn glass_enabled(percent: u8) -> bool {
    clamp_glass_opacity(percent) < GLASS_OPACITY_MAX
}

pub fn glass_opacity_attr(percent: u8) -> String {
    format!("{:.2}", f32::from(clamp_glass_opacity(percent)) / 100.0)
}

#[cfg(test)]
pub fn opacity_to_percent(raw: &str) -> Option<u8> {
    let v: f32 = raw.parse().ok()?;
    if !(0.0..=1.0).contains(&v) {
        return None;
    }
    Some((v * 100.0).round() as u8)
}

fn percent_to_alpha(percent: u8) -> u8 {
    ((u16::from(percent) * 255 + 50) / 100) as u8
}

pub fn glass_accent_opacity(bar_percent: u8) -> u8 {
    clamp_glass_opacity(bar_percent)
}

/// Require exactly one matching node; preserve all unrelated XML and layout.
fn set_tag_attrs(
    xml: &str,
    tag: &str,
    name: Option<&str>,
    attrs: &[(&str, &str)],
) -> Result<String, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut found = None;
    loop {
        let begin = reader.buffer_position() as usize;
        match reader
            .read_event()
            .map_err(|e| format!("XML 格式无效：{e}"))?
        {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                if e.name().as_ref() == tag =>
            {
                let attributes = e
                    .attributes()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                let matches = name.is_none()
                    || attributes.iter().any(|a| {
                        a.key.as_ref() == "name" && name.is_some_and(|n| a.value.as_ref() == n)
                    });
                if matches {
                    if found.is_some() {
                        return Err(format!("XML 中 {tag} {name:?} 重复"));
                    }
                    found = Some((begin, reader.buffer_position() as usize));
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => (),
        }
    }
    let (begin, end) =
        found.ok_or_else(|| format!("官方 window.xml 里找不到 {tag} {name:?}，结构可能已变化"))?;
    let mut text = xml[begin..end].to_string();
    for (attr, value) in attrs {
        let re = Regex::new(&format!(
            r#"\s{}[ \t]*=[ \t]*(?:"[^"]*"|'[^']*')"#,
            regex::escape(attr)
        ))
        .map_err(|e| e.to_string())?;
        let replacement = format!(r#" {attr}="{value}""#);
        if re.is_match(&text) {
            text = re
                .replace(&text, |_: &regex::Captures| replacement.clone())
                .into_owned();
        } else {
            let close = if text.ends_with("/>") { "/>" } else { ">" };
            text = format!(
                "{}{}{}",
                text[..text.len() - close.len()].trim_end(),
                replacement,
                close
            );
        }
    }
    Ok(format!("{}{}{}", &xml[..begin], text, &xml[end..]))
}

/// Preserve official Light colours and layout; replace exactly both font nodes.
fn official_window_xml(official: &str, fonts: &FontChoice) -> Result<String, String> {
    let face = xml_face(&fonts.face);
    let xml = set_tag_attrs(official, "Font", Some("word"), &[("facename", &face)])?;
    set_tag_attrs(&xml, "Font", Some("number"), &[("facename", &face)])
}

/// Theme the user's own window.xml: fonts plus text / selection / scrollbar
/// colours. Everything else, including layout, is left exactly as shipped.
fn themed_window_xml(
    official: &str,
    colors: &ThemeColors,
    fonts: &FontChoice,
    glass: bool,
    glass_opacity: u8,
) -> Result<String, String> {
    let accent_alpha = percent_to_alpha(glass_accent_opacity(glass_opacity));
    let accent = argb(&colors.accent, accent_alpha)?;
    let text = argb(&colors.foreground, 0xf2)?;
    let index = argb(&colors.foreground, 0xb3)?;
    let selected = argb(&colors.emphasis, 0xff)?;
    let thumb = argb(&colors.foreground, 0x4d)?;
    let thumb_hot = argb(&colors.foreground, 0x73)?;

    let mut xml = official_window_xml(official, fonts)?;
    if glass {
        xml = set_tag_attrs(&xml, "Window", None, &[("bkcolor", "#00000000")])?;
    }
    for (class, attrs) in [
        (
            "index_style",
            vec![
                ("textcolor", index.as_str()),
                ("selectedtextcolor", selected.as_str()),
            ],
        ),
        (
            "cand_style",
            vec![
                ("textcolor", text.as_str()),
                ("selectedtextcolor", selected.as_str()),
            ],
        ),
        (
            "more_cand_style",
            vec![
                ("textcolor", text.as_str()),
                ("selectedtextcolor", selected.as_str()),
            ],
        ),
        ("cand_container", vec![("selectedbkcolor", accent.as_str())]),
        (
            "more_cand_container",
            vec![("selectedbkcolor", accent.as_str())],
        ),
        (
            "scroll_style",
            vec![
                ("thumbnormalcolor", thumb.as_str()),
                ("thumbhotcolor", thumb_hot.as_str()),
            ],
        ),
    ] {
        xml = set_tag_attrs(&xml, "Class", Some(class), &attrs)?;
    }
    Ok(xml)
}

/// The official chevron keeps its shape; only the stroke follows the theme.
fn recolor_page_open(src: &str, foreground: &str) -> Result<String, String> {
    let fg = normalize_hex(foreground)?;
    let stroke = Regex::new(r#"stroke="(?:black|#[0-9A-Fa-f]{6})""#).map_err(|e| e.to_string())?;
    let opacity = Regex::new(r#"stroke-opacity="[0-9.]+""#).map_err(|e| e.to_string())?;
    let out = stroke.replace_all(src, format!(r#"stroke="{fg}""#).as_str());
    Ok(opacity
        .replace_all(&out, r#"stroke-opacity="0.72""#)
        .into_owned())
}

fn recolor_svg(src: &str, background: &str, fill_opacity: Option<&str>) -> Result<String, String> {
    let bg = normalize_hex(background)?;
    // Only verified official card faces. Never recolor arbitrary hexadecimal fills
    // (a black shadow may use #000000 instead of the named color).
    let re = Regex::new(r#"fill="(?:white|#[fF]{6})"(?:\s+fill-opacity="[0-9.]+")?"#)
        .map_err(|e| e.to_string())?;
    if re.find_iter(src).count() != 1 {
        return Err("背景 SVG 结构不匹配：需要且仅允许一个官方白色卡片面".into());
    }
    let repl = if let Some(alpha) = fill_opacity {
        format!(r#"fill="{bg}" fill-opacity="{alpha}""#)
    } else {
        format!(r#"fill="{bg}""#)
    };
    Ok(re.replace_all(src, repl).into_owned())
}

const STATUS_WND_FILES: &[&str] = &[
    "status_bar_bg.svg",
    "status_divider.svg",
    "mic_normal.svg",
    "mic_hover.svg",
    "mic_recording.svg",
    "mic_recording_hover.svg",
    "lang_cn_normal.svg",
    "lang_cn_hover.svg",
    "lang_en_normal.svg",
    "lang_en_hover.svg",
    "punct_cn_normal.svg",
    "punct_cn_hover.svg",
    "punct_en_normal.svg",
    "punct_en_hover.svg",
    "half_shape_normal.svg",
    "half_shape_hover.svg",
    "full_shape_normal.svg",
    "full_shape_hover.svg",
];

fn hover_chip(background: &str) -> Result<String, String> {
    let (r, g, b) = hex_rgb(background)?;
    let lift = |c: u8| c.saturating_add(16);
    Ok(format!("#{:02X}{:02X}{:02X}", lift(r), lift(g), lift(b)))
}

fn recolor_status_icon(src: &str, colors: &ThemeColors) -> Result<String, String> {
    let fg = normalize_hex(&colors.foreground)?;
    let accent = normalize_hex(&colors.accent)?;
    let chip = hover_chip(&colors.background)?;
    Ok(src
        .replace(
            r#"fill="black" fill-opacity="0.85""#,
            &format!(r#"fill="{fg}" fill-opacity="0.85""#),
        )
        .replace(r##"fill="#F7F7F7""##, &format!(r#"fill="{chip}""#))
        .replace(r##"fill="#4F84FF""##, &format!(r#"fill="{accent}""#)))
}

fn recolor_status_divider(src: &str, foreground: &str) -> Result<String, String> {
    let fg = normalize_hex(foreground)?;
    Ok(src
        .replace(r#"stroke="black""#, &format!(r#"stroke="{fg}""#))
        .replace(r#"stroke="white""#, &format!(r#"stroke="{fg}""#))
        .replace(r#"stroke-opacity="0.1""#, r#"stroke-opacity="0.18""#))
}

/// `originals` is the user's own untouched skin (live or our in-place backup).
/// Nothing here ships with the app; every file is derived on the spot.
#[cfg(test)]
fn generate_status_wnd(
    originals: &Path,
    dest: &Path,
    colors: &ThemeColors,
    fill_opacity: Option<&str>,
    logo: Option<&[u8]>,
) -> Result<(), String> {
    let dest_dir = dest.join("status_wnd");
    fs::create_dir_all(&dest_dir).map_err(|e| format!("无法创建工具栏主题目录：{e}"))?;
    for name in STATUS_WND_FILES {
        let rel = format!("status_wnd/{name}");
        let raw = skin::read_original_text(originals, &rel)?;
        let out = if *name == "status_bar_bg.svg" {
            recolor_svg(&raw, &colors.background, fill_opacity)?
        } else if *name == "status_divider.svg" {
            recolor_status_divider(&raw, &colors.foreground)?
        } else {
            recolor_status_icon(&raw, colors)?
        };
        fs::write(dest_dir.join(name), out)
            .map_err(|e| format!("写 status_wnd/{name} 失败：{e}"))?;
    }
    let png = if let Some(bytes) = logo {
        bytes.to_vec()
    } else {
        skin::read_original_bytes(originals, "status_wnd/logo.png")?
    };
    fs::write(dest_dir.join("logo.png"), png)
        .map_err(|e| format!("写 status_wnd/logo.png 失败：{e}"))?;
    Ok(())
}

#[derive(Serialize)]
pub struct ToolbarPreview {
    pub logo: Option<Vec<u8>>,
    pub bg: String,
    pub divider: String,
    pub mic: String,
    pub lang: String,
    pub punct: String,
    pub half: String,
}

/// Recolor the user's official toolbar SVGs for the helper preview.
/// Nothing here is stored in the repo.
pub fn generate_toolbar_preview(
    src_dir: &Path,
    colors: &ThemeColors,
    glass: bool,
    glass_opacity: u8,
    keep_official: bool,
    logo: Option<&[u8]>,
) -> Result<ToolbarPreview, String> {
    let colors = colors.clone().normalize()?;
    let opacity = glass.then(|| glass_opacity_attr(glass_opacity));
    let fill_opacity = opacity.as_deref();
    let read = |name: &str| {
        String::from_utf8(crate::safe_fs::read(&src_dir.join(name), 4 * 1024 * 1024)?)
            .map_err(|e| e.to_string())
    };
    let bg_raw = read("status_bar_bg.svg")?;
    let div_raw = read("status_divider.svg")?;
    let mic_raw = read("mic_normal.svg")?;
    let lang_raw = read("lang_cn_normal.svg")?;
    let punct_raw = read("punct_cn_normal.svg")?;
    let half_raw = read("half_shape_normal.svg")?;
    let (bg, divider, mic, lang, punct, half) = if keep_official {
        (bg_raw, div_raw, mic_raw, lang_raw, punct_raw, half_raw)
    } else {
        (
            recolor_svg(&bg_raw, &colors.background, fill_opacity)?,
            recolor_status_divider(&div_raw, &colors.foreground)?,
            recolor_status_icon(&mic_raw, &colors)?,
            recolor_status_icon(&lang_raw, &colors)?,
            recolor_status_icon(&punct_raw, &colors)?,
            recolor_status_icon(&half_raw, &colors)?,
        )
    };
    let logo = if let Some(bytes) = logo {
        Some(bytes.to_vec())
    } else {
        crate::safe_fs::read(&src_dir.join("logo.png"), 4 * 1024 * 1024).ok()
    };
    Ok(ToolbarPreview {
        logo,
        bg,
        divider,
        mic,
        lang,
        punct,
        half,
    })
}

const CARD_SVGS: &[&str] = &[
    "bk_image_1.svg",
    "bk_image_2.svg",
    "bk_image_3.svg",
    "bk_image_4.svg",
    "bk_image_5.svg",
    "bk_image_6.svg",
    "bk_image_7.svg",
    "white_bk.svg",
];

#[cfg(test)]
pub fn generate_theme_dir(
    originals: &Path,
    dest: &Path,
    colors: &ThemeColors,
    fonts: &FontChoice,
    glass: bool,
    glass_opacity: u8,
    logo: Option<&[u8]>,
) -> Result<(), String> {
    let colors = colors.clone().normalize()?;
    let opacity = glass.then(|| glass_opacity_attr(glass_opacity));
    let fill_opacity = opacity.as_deref();
    fs::create_dir_all(dest).map_err(|e| format!("无法创建主题目录：{e}"))?;
    for name in CARD_SVGS {
        let raw = skin::read_original_text(originals, name)?;
        fs::write(
            dest.join(name),
            recolor_svg(&raw, &colors.background, fill_opacity)?,
        )
        .map_err(|e| format!("写 {name} 失败：{e}"))?;
    }
    let chevron = skin::read_original_text(originals, "page_open.svg")?;
    fs::write(
        dest.join("page_open.svg"),
        recolor_page_open(&chevron, &colors.foreground)?,
    )
    .map_err(|e| format!("写 page_open.svg 失败：{e}"))?;
    let official = skin::read_original_text(originals, "window.xml")?;
    fs::write(
        dest.join("window.xml"),
        themed_window_xml(&official, &colors, fonts, glass, glass_opacity)?,
    )
    .map_err(|e| format!("写 window.xml 失败：{e}"))?;
    generate_status_wnd(originals, dest, &colors, fill_opacity, logo)?;
    Ok(())
}

#[cfg(test)]
pub fn generate_light_dir(
    originals: &Path,
    dest: &Path,
    fonts: &FontChoice,
    logo: Option<&[u8]>,
) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("无法创建主题目录：{e}"))?;
    for name in CARD_SVGS.iter().chain(["page_open.svg"].iter()) {
        skin::copy_original(originals, name, &dest.join(name))?;
    }
    let official = skin::read_original_text(originals, "window.xml")?;
    fs::write(
        dest.join("window.xml"),
        official_window_xml(&official, fonts)?,
    )
    .map_err(|e| format!("写 window.xml 失败：{e}"))?;
    let dest_dir = dest.join("status_wnd");
    fs::create_dir_all(&dest_dir).map_err(|e| format!("无法创建工具栏主题目录：{e}"))?;
    for name in STATUS_WND_FILES {
        let rel = format!("status_wnd/{name}");
        skin::copy_original(originals, &rel, &dest_dir.join(name))?;
    }
    if let Some(bytes) = logo {
        fs::write(dest_dir.join("logo.png"), bytes)
            .map_err(|e| format!("写 status_wnd/logo.png 失败：{e}"))?;
    } else {
        skin::copy_original(originals, "status_wnd/logo.png", &dest_dir.join("logo.png"))?;
    }
    Ok(())
}

/// The complete patch is generated in memory in both the UI preflight and helper.
pub fn generate_patch(
    originals: &crate::transaction::Files,
    colors: &ThemeColors,
    fonts: &FontChoice,
    light: bool,
    opacity: u8,
    logo: Option<&[u8]>,
) -> Result<crate::transaction::Files, String> {
    let colors = colors.clone().normalize()?;
    let fonts = fonts.clone().normalize()?;
    let glass = glass_enabled(opacity);
    let keep = light && !glass && colors == light_colors();
    let alpha = glass.then(|| glass_opacity_attr(opacity));
    let mut out = originals.clone();
    let text = |name: &str| -> Result<&str, String> {
        std::str::from_utf8(originals.get(name).ok_or_else(|| format!("缺少 {name}"))?)
            .map_err(|e| e.to_string())
    };
    if !keep {
        for rel in CARD_SVGS {
            out.insert(
                (*rel).into(),
                recolor_svg(text(rel)?, &colors.background, alpha.as_deref())?.into_bytes(),
            );
        }
        out.insert(
            "page_open.svg".into(),
            recolor_page_open(text("page_open.svg")?, &colors.foreground)?.into_bytes(),
        );
        for name in STATUS_WND_FILES {
            let rel = format!("status_wnd/{name}");
            let raw = text(&rel)?;
            let svg = match *name {
                "status_bar_bg.svg" => recolor_svg(raw, &colors.background, alpha.as_deref())?,
                "status_divider.svg" => recolor_status_divider(raw, &colors.foreground)?,
                _ => recolor_status_icon(raw, &colors)?,
            };
            out.insert(rel, svg.into_bytes());
        }
    }
    let xml = if keep {
        official_window_xml(text("window.xml")?, &fonts)?
    } else {
        themed_window_xml(text("window.xml")?, &colors, &fonts, glass, opacity)?
    };
    validate_xml(&xml)?;
    out.insert("window.xml".into(), xml.into_bytes());
    if let Some(bytes) = logo {
        crate::avatar::validate_stored(bytes)?;
        out.insert("status_wnd/logo.png".into(), bytes.to_vec());
    }
    Ok(out)
}

fn validate_xml(xml: &str) -> Result<(), String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(format!("XML 格式无效：{e}")),
            _ => (),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_rewrite_handles_reordered_attributes_and_rejects_duplicates() {
        let input="<Window><Font facename='Old' name='word'/><Font facename='Old' name='number'/></Window>";
        let out = official_window_xml(
            input,
            &FontChoice {
                face: "$1 & Test".into(),
            },
        )
        .unwrap();
        assert_eq!(out.matches("facename=\"$1 &amp; Test\"").count(), 2);
        assert!(official_window_xml(
            "<Font name='word'/><Font name='word'/><Font name='number'/>",
            &FontChoice::default()
        )
        .is_err());
        assert!(official_window_xml("<Font name='word'/>", &FontChoice::default()).is_err());
    }
    use crate::fonts::FontChoice;

    #[test]
    fn mix_preset_uses_requested_colors() {
        let mix = mix_colors();
        assert_eq!(mix.accent, "#7794E4");
        assert_eq!(mix.background, "#3D57D6");
        assert_eq!(mix.foreground, "#FEE5CA");
        assert_eq!(mix.emphasis, "#FFFFFF");
    }

    /// Same shape as the shipped file: multi-line Class tags, official colours.
    const OFFICIAL_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<Window caption="wholewindow">
    <Font name="word" facename="Microsoft YaHei" size="19"/>
    <Font name="number" facename="Microsoft YaHei" size="16"/>
    <Class name="index_style" font="number" textcolor="#73000000" selectedtextcolor="#FFFFFFFF"/>
    <Class name="cand_style" width="auto" font="word" textcolor="#BF000000" selectedtextcolor="#FFFFFFFF"/>
    <Class name="more_cand_style" align="left|vcenter" font="word" textcolor="#BF000000" selectedtextcolor="#FFFFFFFF"/>
    <Class name="cand_container" width="auto" mousechild="false" 
            selectedbkcolor="#FF4F84FF"/>
    <Class name="more_cand_container" maxwidth="matchparent" 
            selectedbkcolor="#FF4F84FF" />
    <Class name="scroll_style" width="9" thumbnormalcolor="#14000000" thumbhotcolor="#28000000" bkround="5"/>
    <TabLayout width="auto" bkimage="file='white_bk.svg' corner='40,40,40,40'"/>
</Window>
"##;

    #[test]
    fn selected_text_follows_emphasis() {
        let mut colors = dark_colors();
        colors.emphasis = "#FEE5CA".into();
        let xml = themed_window_xml(
            OFFICIAL_XML,
            &colors,
            &FontChoice::default(),
            false,
            GLASS_OPACITY_DEFAULT,
        )
        .unwrap();
        assert!(xml.contains("selectedtextcolor=\"#FFFEE5CA\""));
        assert!(!xml.contains("selectedtextcolor=\"#FFFFFFFF\""));
    }

    #[test]
    fn themed_xml_patches_colors_and_keeps_layout() {
        let xml = themed_window_xml(
            OFFICIAL_XML,
            &dark_colors(),
            &FontChoice {
                face: "KaiTi".into(),
            },
            false,
            GLASS_OPACITY_DEFAULT,
        )
        .unwrap();
        assert!(xml.contains(r#"<Font name="word" facename="KaiTi" size="19"/>"#));
        assert!(xml.contains(r#"<Font name="number" facename="KaiTi" size="16"/>"#));
        assert!(xml.contains(r##"<Class name="index_style" font="number" textcolor="#B3F2F2F2" selectedtextcolor="#FFFFFFFF"/>"##));
        assert!(xml.contains(r##"textcolor="#F2F2F2F2""##));
        assert!(xml.contains("selectedbkcolor=\"#FF8A8A8A\"/>"));
        assert!(xml.contains("selectedbkcolor=\"#FF8A8A8A\" />"));
        assert!(xml.contains(r##"thumbnormalcolor="#4DF2F2F2" thumbhotcolor="#73F2F2F2""##));
        assert!(xml.contains(r#"<Window caption="wholewindow">"#));
        assert!(xml.contains("bkimage=\"file='white_bk.svg' corner='40,40,40,40'\""));
        assert!(!xml.contains("#BF000000") && !xml.contains("#FF4F84FF"));
        let missing = OFFICIAL_XML.replace("name=\"scroll_style\"", "name=\"other\"");
        assert!(
            themed_window_xml(&missing, &dark_colors(), &FontChoice::default(), false, 70).is_err()
        );
    }

    #[test]
    fn page_open_keeps_shape_and_follows_foreground() {
        let src = r#"<svg><path d="M4 7L10 12L15 7" stroke="black" stroke-opacity="0.45" stroke-width="1.3"/></svg>"#;
        let out = recolor_page_open(src, "#F2F2F2").unwrap();
        assert!(out.contains(r#"d="M4 7L10 12L15 7""#));
        assert!(out.contains(r##"stroke="#F2F2F2" stroke-opacity="0.72""##));
    }

    /// A stand-in for the user's untouched skin: same file names, minimal bodies.
    fn fake_originals(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("dmdm-orig-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("status_wnd")).unwrap();
        for name in CARD_SVGS {
            fs::write(
                root.join(name),
                r#"<svg><rect fill="black" fill-opacity="0.08"/><rect fill="white"/></svg>"#,
            )
            .unwrap();
        }
        fs::write(
            root.join("page_open.svg"),
            r#"<svg><path d="M4 7L10 12L15 7" stroke="black" stroke-opacity="0.45"/></svg>"#,
        )
        .unwrap();
        fs::write(root.join("window.xml"), OFFICIAL_XML).unwrap();
        for name in STATUS_WND_FILES {
            fs::write(
                root.join("status_wnd").join(name),
                r##"<svg><rect fill="white"/><path fill="black" fill-opacity="0.85"/><rect fill="#F7F7F7"/><rect fill="#4F84FF"/><line stroke="black" stroke-opacity="0.1"/></svg>"##,
            )
            .unwrap();
        }
        // 2x2 white PNG stands in for the official avatar.
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&[255u8; 16])
                .unwrap();
        }
        fs::write(root.join("status_wnd/logo.png"), png).unwrap();
        root
    }

    #[test]
    fn light_xml_swaps_only_the_faces() {
        let xml = official_window_xml(
            OFFICIAL_XML,
            &FontChoice {
                face: "幼圆".into(),
            },
        )
        .unwrap();
        assert!(xml.contains("<Font name=\"word\" facename=\"幼圆\" size=\"19\"/>"));
        assert!(xml.contains("<Font name=\"number\" facename=\"幼圆\" size=\"16\"/>"));
        assert!(xml.contains("selectedbkcolor=\"#FF4F84FF\""));
        assert!(xml.contains("textcolor=\"#BF000000\""));
        assert!(official_window_xml("<Window/>", &FontChoice::default()).is_err());
    }

    #[test]
    fn light_dir_keeps_official_files() {
        let originals = fake_originals("light");
        let dest = originals.join("out");
        generate_light_dir(
            &originals,
            &dest,
            &FontChoice {
                face: "幼圆".into(),
            },
            None,
        )
        .unwrap();
        let svg = fs::read_to_string(dest.join("bk_image_2.svg")).unwrap();
        assert!(svg.contains(r#"fill="white""#));
        let xml = fs::read_to_string(dest.join("window.xml")).unwrap();
        assert!(xml.contains("facename=\"幼圆\""));
        assert!(xml.contains("selectedbkcolor=\"#FF4F84FF\""));
        assert_eq!(
            fs::read(dest.join("status_wnd/logo.png")).unwrap(),
            fs::read(originals.join("status_wnd/logo.png")).unwrap()
        );
        let _ = fs::remove_dir_all(&originals);
    }

    #[test]
    fn light_dir_writes_logo_override() {
        let originals = fake_originals("light-logo");
        let dest = originals.join("out");
        let mark = b"user-square-png";
        generate_light_dir(
            &originals,
            &dest,
            &FontChoice {
                face: "幼圆".into(),
            },
            Some(mark),
        )
        .unwrap();
        assert_eq!(fs::read(dest.join("status_wnd/logo.png")).unwrap(), mark);
        let _ = fs::remove_dir_all(&originals);
    }

    #[test]
    fn dark_dir_recolors_everything_from_originals() {
        let originals = fake_originals("dark");
        let dest = originals.join("out");
        generate_theme_dir(
            &originals,
            &dest,
            &dark_colors(),
            &FontChoice::default(),
            false,
            GLASS_OPACITY_DEFAULT,
            None,
        )
        .unwrap();
        let svg = fs::read_to_string(dest.join("bk_image_2.svg")).unwrap();
        assert!(svg.contains("fill=\"#2A2A2A\""));
        assert!(svg.contains("fill=\"black\" fill-opacity=\"0.08\""));
        let bar = fs::read_to_string(dest.join("status_wnd/status_bar_bg.svg")).unwrap();
        assert!(bar.contains("fill=\"#2A2A2A\""));
        let icon = fs::read_to_string(dest.join("status_wnd/mic_normal.svg")).unwrap();
        assert!(icon.contains("fill=\"#F2F2F2\" fill-opacity=\"0.85\""));
        assert!(icon.contains("fill=\"#8A8A8A\""));
        let chevron = fs::read_to_string(dest.join("page_open.svg")).unwrap();
        assert!(chevron.contains("stroke=\"#F2F2F2\" stroke-opacity=\"0.72\""));
        let xml = fs::read_to_string(dest.join("window.xml")).unwrap();
        assert!(xml.contains("selectedbkcolor=\"#FF8A8A8A\""));
        assert!(xml.contains("<TabLayout width=\"auto\""));
        assert_eq!(
            fs::read(dest.join("status_wnd/logo.png")).unwrap(),
            fs::read(originals.join("status_wnd/logo.png")).unwrap()
        );
        let _ = fs::remove_dir_all(&originals);
    }

    #[test]
    fn logo_override_is_written_verbatim() {
        let originals = fake_originals("logo-override");
        let dest = originals.join("out");
        let mark = b"not-a-png-just-bytes";
        generate_theme_dir(
            &originals,
            &dest,
            &dark_colors(),
            &FontChoice::default(),
            false,
            GLASS_OPACITY_DEFAULT,
            Some(mark),
        )
        .unwrap();
        assert_eq!(fs::read(dest.join("status_wnd/logo.png")).unwrap(), mark);
        let _ = fs::remove_dir_all(&originals);
    }

    #[test]
    fn toolbar_preview_recolors_and_keeps_official() {
        let originals = fake_originals("toolbar-preview");
        let src = originals.join("status_wnd");
        let dark = generate_toolbar_preview(
            &src,
            &dark_colors(),
            false,
            GLASS_OPACITY_DEFAULT,
            false,
            None,
        )
        .unwrap();
        assert!(dark.bg.contains("fill=\"#2A2A2A\""));
        assert!(dark.mic.contains("fill=\"#F2F2F2\" fill-opacity=\"0.85\""));
        assert_eq!(
            dark.logo.as_ref().unwrap(),
            &fs::read(src.join("logo.png")).unwrap()
        );
        let light = generate_toolbar_preview(
            &src,
            &light_colors(),
            false,
            GLASS_OPACITY_DEFAULT,
            true,
            Some(b"user"),
        )
        .unwrap();
        assert!(light.bg.contains(r#"fill="white""#));
        assert_eq!(light.logo.as_deref(), Some(b"user".as_slice()));
        let _ = fs::remove_dir_all(&originals);
    }

    #[test]
    fn glass_svg_keeps_card_alpha() {
        let src = r##"<rect fill="white"/><rect fill="black" fill-opacity="0.08"/><rect fill="#000000"/>"##;
        let out = recolor_svg(src, "#2A2A2A", Some(&glass_opacity_attr(70))).unwrap();
        assert!(out.contains("fill=\"#2A2A2A\" fill-opacity=\"0.70\""));
        assert!(out.contains("fill=\"black\" fill-opacity=\"0.08\""));
        assert!(out.contains(r##"fill="#000000""##));
        let thinner = recolor_svg(src, "#2A2A2A", Some(&glass_opacity_attr(40))).unwrap();
        assert!(thinner.contains("fill=\"#2A2A2A\" fill-opacity=\"0.40\""));
        assert_eq!(clamp_glass_opacity(3), 20);
        assert_eq!(clamp_glass_opacity(100), 100);
        assert!(!glass_enabled(100));
        assert!(glass_enabled(70));
        assert_eq!(opacity_to_percent("0.70"), Some(70));
        assert_eq!(glass_accent_opacity(100), 100);
        assert_eq!(glass_accent_opacity(99), 99);
        assert_eq!(glass_accent_opacity(70), 70);
        assert_eq!(glass_accent_opacity(40), 40);
        let xml = themed_window_xml(
            OFFICIAL_XML,
            &dark_colors(),
            &FontChoice {
                face: "KaiTi".into(),
            },
            true,
            70,
        )
        .unwrap();
        assert!(xml.contains(r##"<Window caption="wholewindow" bkcolor="#00000000">"##));
        assert!(xml.contains("selectedbkcolor=\"#B38A8A8A\""));
        let thinner_xml = themed_window_xml(
            OFFICIAL_XML,
            &dark_colors(),
            &FontChoice {
                face: "KaiTi".into(),
            },
            true,
            40,
        )
        .unwrap();
        assert!(thinner_xml.contains("selectedbkcolor=\"#668A8A8A\""));
        let almost = recolor_svg(src, "#2A2A2A", Some(&glass_opacity_attr(99))).unwrap();
        assert!(almost.contains("fill-opacity=\"0.99\""));
        assert!(recolor_svg(
            r##"<rect fill="#2A2A2A" fill-opacity="0.96"/>"##,
            "#FFFFFF",
            Some(&glass_opacity_attr(80))
        )
        .is_err());
        let opaque = recolor_svg(src, "#2A2A2A", None).unwrap();
        assert!(opaque.contains("fill=\"#2A2A2A\""));
        assert!(!opaque.contains("fill-opacity=\"1.00\""));
    }
}
