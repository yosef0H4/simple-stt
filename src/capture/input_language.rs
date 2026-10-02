#[cfg(not(windows))]
use anyhow::bail;
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeyboardLanguage {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyboardLanguages {
    pub available: bool,
    pub message: String,
    pub languages: Vec<KeyboardLanguage>,
}

pub fn keyboard_languages() -> KeyboardLanguages {
    match configured_languages() {
        Ok(languages) => KeyboardLanguages {
            available: true,
            message: String::new(),
            languages: deduplicate(languages),
        },
        Err(error) => KeyboardLanguages {
            available: false,
            message: format!(
                "Keyboard language detection unavailable: {error:#}. Use one model instead."
            ),
            languages: Vec::new(),
        },
    }
}

fn deduplicate(languages: Vec<KeyboardLanguage>) -> Vec<KeyboardLanguage> {
    languages
        .into_iter()
        .map(|language| (language.id.clone(), language))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

pub fn normalize_language(code: &str) -> Option<KeyboardLanguage> {
    let primary = code.split(['-', '_']).next()?.to_ascii_lowercase();
    let language = isolang::Language::from_639_1(&primary)
        .or_else(|| isolang::Language::from_639_3(&primary))?;
    Some(KeyboardLanguage {
        id: language.to_639_1().unwrap_or(language.to_639_3()).into(),
        name: language.to_name().into(),
    })
}

#[cfg(windows)]
fn windows_language(layout: usize) -> KeyboardLanguage {
    use windows_sys::Win32::Globalization::{GetLocaleInfoW, LOCALE_SISO639LANGNAME};
    let lang_id = (layout & 0xffff) as u32;
    let mut buffer = [0_u16; 85];
    let length = unsafe {
        GetLocaleInfoW(
            lang_id,
            LOCALE_SISO639LANGNAME,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    if length > 1 {
        if let Some(language) =
            normalize_language(&String::from_utf16_lossy(&buffer[..length as usize - 1]))
        {
            return language;
        }
    }
    KeyboardLanguage {
        id: format!("win:{lang_id:04x}"),
        name: format!("Keyboard language {lang_id:04x}"),
    }
}

#[cfg(windows)]
fn configured_languages() -> Result<Vec<KeyboardLanguage>> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayoutList;
    let count = unsafe { GetKeyboardLayoutList(0, std::ptr::null_mut()) };
    anyhow::ensure!(count > 0, "Cannot list keyboard layouts");
    let mut layouts = vec![std::ptr::null_mut(); count as usize];
    let count = unsafe { GetKeyboardLayoutList(count, layouts.as_mut_ptr()) };
    anyhow::ensure!(count > 0, "Cannot list keyboard layouts");
    Ok(layouts[..count as usize]
        .iter()
        .map(|layout| windows_language(*layout as usize))
        .collect())
}

#[cfg(windows)]
pub fn active_keyboard_language() -> Result<String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let window = GetForegroundWindow();
        anyhow::ensure!(!window.is_null(), "No foreground window");
        let thread = GetWindowThreadProcessId(window, std::ptr::null_mut());
        anyhow::ensure!(thread != 0, "Cannot read foreground keyboard language");
        Ok(windows_language(GetKeyboardLayout(thread) as usize).id)
    }
}

#[cfg(target_os = "linux")]
fn wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(target_os = "linux")]
type LayoutInfo = (String, String, String);
#[cfg(target_os = "linux")]
type LayoutState = (usize, Vec<LayoutInfo>);

#[cfg(target_os = "linux")]
fn kde_layouts() -> Result<LayoutState> {
    futures_lite::future::block_on(async {
        let query = async {
            let connection = ashpd::zbus::Connection::session().await?;
            let proxy = ashpd::zbus::Proxy::new(
                &connection,
                "org.kde.keyboard",
                "/Layouts",
                "org.kde.KeyboardLayouts",
            )
            .await?;
            let index: u32 = proxy.call("getLayout", &()).await?;
            let layouts: Vec<(String, String, String)> = proxy.call("getLayoutsList", &()).await?;
            Ok((index as usize, layouts))
        };
        futures_lite::future::race(query, async {
            async_io::Timer::after(std::time::Duration::from_secs(1)).await;
            bail!("KDE keyboard query timed out")
        })
        .await
    })
}

#[cfg(target_os = "linux")]
fn x11_layouts() -> Result<LayoutState> {
    use x11rb::connection::Connection;
    use x11rb::protocol::{
        xkb,
        xproto::{AtomEnum, ConnectionExt},
    };
    let (connection, screen) = x11rb::connect(None)?;
    anyhow::ensure!(
        xkb::use_extension(&connection, 1, 0)?.reply()?.supported,
        "XKB unavailable"
    );
    let group = xkb::get_state(&connection, xkb::ID::USE_CORE_KBD.into())?
        .reply()?
        .group;
    let atom = connection
        .intern_atom(true, b"_XKB_RULES_NAMES")?
        .reply()?
        .atom;
    let property = connection
        .get_property(
            false,
            connection.setup().roots[screen].root,
            atom,
            AtomEnum::STRING,
            0,
            1024,
        )?
        .reply()?;
    Ok((
        u8::from(group) as usize,
        layouts_from_rules(&property.value)?,
    ))
}

#[cfg(target_os = "linux")]
fn layouts_from_rules(rules: &[u8]) -> Result<Vec<LayoutInfo>> {
    let fields = rules.split(|byte| *byte == 0).collect::<Vec<_>>();
    let layouts = std::str::from_utf8(fields.get(2).context("XKB layouts missing")?)?;
    anyhow::ensure!(!layouts.is_empty(), "XKB layouts empty");
    let variants = std::str::from_utf8(fields.get(3).copied().unwrap_or_default())?
        .split(',')
        .collect::<Vec<_>>();
    Ok(layouts
        .split(',')
        .enumerate()
        .map(|(index, layout)| {
            (
                layout.into(),
                variants.get(index).copied().unwrap_or_default().into(),
                layout.into(),
            )
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn linux_layouts() -> Result<LayoutState> {
    if wayland_session() {
        kde_layouts()
    } else {
        x11_layouts()
    }
}

#[cfg(target_os = "linux")]
fn xkb_metadata() -> Option<String> {
    let root = std::env::var_os("XKB_CONFIG_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/usr/share/X11/xkb".into());
    std::fs::read_to_string(root.join("rules/evdev.xml")).ok()
}

#[cfg(target_os = "linux")]
fn language_from_layout(
    layout: &str,
    variant: &str,
    label: &str,
    xml: Option<&str>,
) -> KeyboardLanguage {
    let detected = xml.and_then(|xml| {
        let document = roxmltree::Document::parse(xml).ok()?;
        let matches_name = |node: roxmltree::Node<'_, '_>, name: &str| {
            node.children()
                .find(|child| child.has_tag_name("configItem"))
                .and_then(|item| item.children().find(|child| child.has_tag_name("name")))
                .and_then(|name_node| name_node.text())
                == Some(name)
        };
        let layout_node = document
            .descendants()
            .find(|node| node.has_tag_name("layout") && matches_name(*node, layout))?;
        let variant_item = layout_node
            .descendants()
            .find(|node| node.has_tag_name("variant") && matches_name(*node, variant))
            .and_then(|node| {
                node.children()
                    .find(|child| child.has_tag_name("configItem"))
            });
        let primary_item = layout_node
            .children()
            .find(|child| child.has_tag_name("configItem"))?;
        let iso = |node: roxmltree::Node<'_, '_>| {
            node.descendants()
                .find(|child| child.has_tag_name("iso639Id"))
                .and_then(|node| node.text())
                .and_then(normalize_language)
        };
        variant_item.and_then(iso).or_else(|| iso(primary_item))
    });
    detected
        .or_else(|| match layout {
            "us" | "gb" | "au" => normalize_language("en"),
            "ara" => normalize_language("ar"),
            _ => None,
        })
        .unwrap_or_else(|| KeyboardLanguage {
            id: format!("xkb:{layout}:{variant}"),
            name: if label.is_empty() {
                layout.into()
            } else {
                label.into()
            },
        })
}

#[cfg(target_os = "linux")]
fn configured_languages() -> Result<Vec<KeyboardLanguage>> {
    let (_, layouts) = linux_layouts()?;
    let xml = xkb_metadata();
    Ok(layouts
        .iter()
        .map(|(layout, variant, label)| {
            language_from_layout(layout, variant, label, xml.as_deref())
        })
        .collect())
}

#[cfg(target_os = "linux")]
pub fn active_keyboard_language() -> Result<String> {
    let (group, layouts) = linux_layouts()?;
    let (layout, variant, label) = layouts
        .get(group)
        .context("Active keyboard group unavailable")?;
    Ok(language_from_layout(layout, variant, label, xkb_metadata().as_deref()).id)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn configured_languages() -> Result<Vec<KeyboardLanguage>> {
    bail!("Unsupported desktop")
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn active_keyboard_language() -> Result<String> {
    bail!("Unsupported desktop")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regional_languages_are_deduplicated() {
        let languages = deduplicate(vec![
            normalize_language("en-US").unwrap(),
            normalize_language("en_GB").unwrap(),
            normalize_language("ara").unwrap(),
            normalize_language("fra").unwrap(),
        ]);
        assert_eq!(
            languages
                .iter()
                .map(|language| language.id.as_str())
                .collect::<Vec<_>>(),
            ["ar", "en", "fr"]
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn layout_metadata_variant_and_unknown_identity() {
        let xml = r#"<root><layout><configItem><name>custom</name><languageList><iso639Id>eng</iso639Id></languageList></configItem><variantList><variant><configItem><name>arabic</name><languageList><iso639Id>ara</iso639Id></languageList></configItem></variant></variantList></layout></root>"#;
        assert_eq!(language_from_layout("custom", "", "", Some(xml)).id, "en");
        assert_eq!(
            language_from_layout("custom", "arabic", "", Some(xml)).id,
            "ar"
        );
        assert_eq!(
            language_from_layout("unknown", "variant", "My layout", Some(xml)).id,
            "xkb:unknown:variant"
        );
        let rules = b"evdev\0pc105\0us,ara\0,\0";
        assert_eq!(layouts_from_rules(rules).unwrap()[1].0, "ara");
        assert!(layouts_from_rules(b"").is_err());
    }
    #[test]
    fn live_keyboard_language() {
        let Ok(expected) = std::env::var("SIMPLE_STT_TEST_KEYBOARD_LANGUAGE") else {
            return;
        };
        assert_eq!(active_keyboard_language().unwrap(), expected);
    }
}
