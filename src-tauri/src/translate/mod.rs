use crate::models::OcrRegion;
use async_trait::async_trait;
use thiserror::Error;
use zhconv::{zhconv, Variant};

#[derive(Error, Debug)]
pub enum TranslateError {
    #[error("Translator not available: {0}")]
    NotAvailable(String),
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait TranslateProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn available(&self) -> bool {
        true
    }
    async fn translate(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, TranslateError>;
}

pub fn looks_japanese(text: &str) -> bool {
    // Hiragana, or katakana *letters*. U+30FB (・) and neighbors are punctuation Vision
    // sprinkles onto Chinese when Japanese is in the language list; those must not count.
    text.chars().any(|c| {
        let u = c as u32;
        (0x3040..=0x309F).contains(&u) || (0x30A1..=0x30FA).contains(&u)
    })
}

pub fn looks_korean(text: &str) -> bool {
    text.chars().any(|c| {
        let u = c as u32;
        (0xAC00..=0xD7AF).contains(&u) || (0x1100..=0x11FF).contains(&u)
    })
}

pub fn looks_han(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF))
}

pub struct OpenCcProvider;
pub struct AppleTranslateProvider;
pub struct CloudTranslateProvider;

impl OpenCcProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TranslateProvider for OpenCcProvider {
    fn id(&self) -> &str {
        "opencc"
    }
    fn name(&self) -> &str {
        "OpenCC (Simplified → Traditional)"
    }
    async fn translate(
        &self,
        text: &str,
        _source_lang: &str,
        _target_lang: &str,
    ) -> Result<String, TranslateError> {
        Ok(convert_han(text))
    }
}

impl AppleTranslateProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TranslateProvider for AppleTranslateProvider {
    fn id(&self) -> &str {
        "apple"
    }
    fn name(&self) -> &str {
        "Apple Translation"
    }
    fn available(&self) -> bool {
        cfg!(target_os = "macos")
    }
    async fn translate(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, TranslateError> {
        Ok(convert_local_with_apple(text, source_lang, target_lang))
    }
}

impl CloudTranslateProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TranslateProvider for CloudTranslateProvider {
    fn id(&self) -> &str {
        "cloud"
    }
    fn name(&self) -> &str {
        "Cloud Translation"
    }
    fn available(&self) -> bool {
        false
    }
    async fn translate(
        &self,
        _text: &str,
        _source_lang: &str,
        _target_lang: &str,
    ) -> Result<String, TranslateError> {
        Err(TranslateError::NotAvailable(
            "Cloud translation is not configured".into(),
        ))
    }
}

pub fn list_providers() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        ("apple", "Apple Translation", cfg!(target_os = "macos")),
        ("opencc", "OpenCC (Simplified → Traditional)", true),
        ("cloud", "Cloud Translation", false),
    ]
}

pub fn get_provider(id: &str) -> Box<dyn TranslateProvider> {
    match id {
        "cloud" => Box::new(CloudTranslateProvider::new()),
        "opencc" => Box::new(OpenCcProvider::new()),
        _ => Box::new(AppleTranslateProvider::new()),
    }
}

pub fn convert_han(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    if looks_japanese(text) || looks_korean(text) {
        return text.to_string();
    }
    if looks_han(text) {
        return zhconv(text, Variant::ZhTW);
    }
    text.to_string()
}

fn convert_local_with_apple(text: &str, _source: &str, _target: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    if looks_han(text) && !looks_japanese(text) && !looks_korean(text) {
        return zhconv(text, Variant::ZhTW);
    }
    if looks_japanese(text) || looks_korean(text) || looks_latin(text) {
        if let Some(out) = apple_translate(text) {
            return out;
        }
    }
    if looks_han(text) {
        return zhconv(text, Variant::ZhTW);
    }
    text.to_string()
}

fn looks_latin(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    !letters.is_empty() && letters.iter().all(|c| c.is_ascii_alphabetic())
}

pub fn run_translate_sync(id: &str, regions: &[OcrRegion]) -> Result<Vec<OcrRegion>, TranslateError> {
    if id == "cloud" {
        return Err(TranslateError::NotAvailable(
            "Cloud translation is not configured".into(),
        ));
    }
    let use_apple = id != "opencc";
    let mut out = Vec::new();
    for region in regions {
        if region.hidden {
            out.push(region.clone());
            continue;
        }
        let translated = if use_apple {
            convert_local_with_apple(&region.text, "", "zh-Hant")
        } else {
            convert_han(&region.text)
        };
        out.push(OcrRegion {
            translated_text: Some(translated),
            ..region.clone()
        });
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
fn apple_translate(text: &str) -> Option<String> {
    apple_ffi::translate(text)
}

#[cfg(not(target_os = "macos"))]
fn apple_translate(_text: &str) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
mod apple_ffi {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    unsafe extern "C" {
        fn shelf_apple_translate(text: *const c_char, out: *mut *mut c_char) -> i32;
        fn shelf_apple_translate_free(ptr: *mut c_char);
    }

    pub fn translate(text: &str) -> Option<String> {
        let c_text = CString::new(text).ok()?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { shelf_apple_translate(c_text.as_ptr(), &mut out) };
        if rc != 0 || out.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { shelf_apple_translate_free(out) };
        if s.is_empty() || s == text {
            None
        } else {
            Some(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_requires_kana_letters_not_punctuation() {
        assert!(looks_japanese("こんにちは"));
        assert!(looks_japanese("カタカナ"));
        assert!(!looks_japanese("你的接吻实力"));
        assert!(!looks_japanese("…・・呃・"));
        assert!(!looks_japanese(".你的接吻安力"));
    }

    #[test]
    fn han_conversion_uses_taiwan_traditional() {
        assert_eq!(convert_han("你的接吻实力"), "你的接吻實力");
        assert_eq!(convert_han("好像有点生疏了。"), "好像有點生疏了。");
        assert_eq!(convert_han("こんにちは"), "こんにちは");
    }
}
