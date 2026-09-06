use crate::models::OcrRegion;
use async_trait::async_trait;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OcrError {
    #[error("OCR engine not available: {0}")]
    NotAvailable(String),
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait OcrEngine: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn available(&self) -> bool {
        false
    }
    async fn recognize(&self, image_bytes: &[u8], languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError>;
}

pub struct AppleVisionEngine;
pub struct MangaOcrEngine;
pub struct PaddleOcrEngine;
pub struct RapidOcrEngine;

impl AppleVisionEngine {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
mod vision_ffi {
    use crate::models::OcrRegion;
    use crate::ocr::OcrError;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::os::raw::c_ulong;
    use uuid::Uuid;

    type Cb = extern "C" fn(*mut c_void, f64, f64, f64, f64, *const c_char, i32);

    unsafe extern "C" {
        fn shelf_vision_ocr(
            data: *const u8,
            len: c_ulong,
            lang_csv: *const c_char,
            cb: Cb,
            ctx: *mut c_void,
        ) -> i32;
    }

    struct Collect(Vec<OcrRegion>);

    extern "C" fn collect(
        ctx: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        text: *const c_char,
        vertical: i32,
    ) {
        let collector = unsafe { &mut *(ctx as *mut Collect) };
        let text = if text.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
        };
        let text = crate::ocr::cleanup_ocr_text(&text);
        if text.is_empty() {
            return;
        }
        collector.0.push(OcrRegion {
            id: Uuid::new_v4().to_string(),
            x,
            y,
            width: w,
            height: h,
            text,
            translated_text: None,
            vertical: vertical != 0,
            hidden: false,
        });
    }

    pub fn recognize(image_bytes: &[u8], languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError> {
        let langs = languages.join(",");
        let c_langs = CString::new(langs).unwrap_or_default();
        let mut collector = Collect(Vec::new());
        let rc = unsafe {
            shelf_vision_ocr(
                image_bytes.as_ptr(),
                image_bytes.len() as c_ulong,
                c_langs.as_ptr(),
                collect,
                &mut collector as *mut Collect as *mut c_void,
            )
        };
        if rc != 0 {
            return Err(OcrError::Other(format!("Apple Vision failed ({rc})")));
        }
        Ok(collector.0)
    }
}

#[async_trait]
impl OcrEngine for AppleVisionEngine {
    fn id(&self) -> &str {
        "apple_vision"
    }
    fn name(&self) -> &str {
        "Apple Vision"
    }
    fn available(&self) -> bool {
        cfg!(target_os = "macos")
    }
    async fn recognize(&self, image_bytes: &[u8], languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError> {
        #[cfg(target_os = "macos")]
        {
            vision_ffi::recognize(image_bytes, languages)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (image_bytes, languages);
            Err(OcrError::NotAvailable("Apple Vision is macOS-only".into()))
        }
    }
}

impl MangaOcrEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl OcrEngine for MangaOcrEngine {
    fn id(&self) -> &str {
        "manga_ocr"
    }
    fn name(&self) -> &str {
        "Manga OCR"
    }
    async fn recognize(&self, _image_bytes: &[u8], _languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError> {
        Err(OcrError::NotAvailable(
            "Manga OCR is not installed yet — select it after benchmarking".into(),
        ))
    }
}

impl PaddleOcrEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl OcrEngine for PaddleOcrEngine {
    fn id(&self) -> &str {
        "paddle_ocr"
    }
    fn name(&self) -> &str {
        "PaddleOCR"
    }
    async fn recognize(&self, _image_bytes: &[u8], _languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError> {
        Err(OcrError::NotAvailable(
            "PaddleOCR is not installed yet — select it after benchmarking".into(),
        ))
    }
}

impl RapidOcrEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl OcrEngine for RapidOcrEngine {
    fn id(&self) -> &str {
        "rapid_ocr"
    }
    fn name(&self) -> &str {
        "RapidOCR"
    }
    async fn recognize(&self, _image_bytes: &[u8], _languages: &[&str]) -> Result<Vec<OcrRegion>, OcrError> {
        Err(OcrError::NotAvailable(
            "RapidOCR is not installed yet — select it after benchmarking".into(),
        ))
    }
}

pub fn list_engines() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        ("apple_vision", "Apple Vision", cfg!(target_os = "macos")),
        ("manga_ocr", "Manga OCR", false),
        ("paddle_ocr", "PaddleOCR", false),
        ("rapid_ocr", "RapidOCR", false),
    ]
}

pub fn get_engine(id: &str) -> Box<dyn OcrEngine> {
    match id {
        "manga_ocr" => Box::new(MangaOcrEngine::new()),
        "paddle_ocr" => Box::new(PaddleOcrEngine::new()),
        "rapid_ocr" => Box::new(RapidOcrEngine::new()),
        _ => Box::new(AppleVisionEngine::new()),
    }
}

pub fn cleanup_ocr_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut out = trimmed.to_string();
    // Vision often prefixes a bubble tail as ASCII '.' or Japanese '・'.
    let leading = ['。', '.', '·', '・', '･', '•', ',', '，', '…', '⋯', '：', ':', '「', '」'];
    while out.chars().next().is_some_and(|c| leading.contains(&c)) {
        let rest: String = out.chars().skip(1).collect();
        if rest.chars().any(|c| !leading.contains(&c) && !c.is_whitespace()) {
            out = rest.trim_start().to_string();
        } else {
            break;
        }
    }

    if crate::translate::looks_han(&out) && !crate::translate::looks_japanese(&out) {
        out = out.replace(['・', '･'], "");
        if out.ends_with('.') && !out.ends_with("..") {
            out.pop();
            out.push('。');
        }
    }
    out.trim().to_string()
}

pub fn run_ocr_sync(engine_id: &str, image_bytes: &[u8]) -> Result<Vec<OcrRegion>, OcrError> {
    let png = image_to_png(image_bytes).unwrap_or_else(|_| image_bytes.to_vec());
    match engine_id {
        "manga_ocr" => Err(OcrError::NotAvailable(
            "Manga OCR is not installed yet — select it after benchmarking".into(),
        )),
        "paddle_ocr" => Err(OcrError::NotAvailable(
            "PaddleOCR is not installed yet — select it after benchmarking".into(),
        )),
        "rapid_ocr" => Err(OcrError::NotAvailable(
            "RapidOCR is not installed yet — select it after benchmarking".into(),
        )),
        _ => {
            #[cfg(target_os = "macos")]
            {
                vision_ffi::recognize(&png, &["zh-Hans", "zh-Hant", "ja-JP", "ko-KR", "en-US"])
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = png;
                Err(OcrError::NotAvailable("Apple Vision is macOS-only".into()))
            }
        }
    }
}

fn image_to_png(bytes: &[u8]) -> Result<Vec<u8>, OcrError> {
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| OcrError::Other(e.to_string()))?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| OcrError::Other(e.to_string()))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_strips_bubble_tails_from_chinese() {
        assert_eq!(cleanup_ocr_text(".你的接吻实力"), "你的接吻实力");
        assert_eq!(cleanup_ocr_text("：你的接吻实力"), "你的接吻实力");
        assert_eq!(cleanup_ocr_text("好像有点生疏了."), "好像有点生疏了。");
        assert_eq!(cleanup_ocr_text("…・・呃・"), "呃");
    }

    #[test]
    fn cleanup_keeps_japanese_words() {
        assert_eq!(cleanup_ocr_text("こんにちは"), "こんにちは");
    }
}

