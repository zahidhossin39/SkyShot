//! AI re-read: send ONE selection to Claude or Gemini with the user's own API key and get a
//! better transcription than the offline OCR (blurry text, handwriting, odd fonts).
//! No server of ours in between. The key lives in Windows Credential Manager, not settings.json.
//!
//! Request building and response parsing are pure (unit-tested); `post` is the only I/O.

use serde_json::{json, Value};

pub const PROVIDERS: [&str; 2] = ["claude", "gemini"];

const PROMPT: &str = "Transcribe all text in this screenshot exactly as written. Keep the original \
line breaks and reading order. Do not translate, summarize, fix, or add anything. If a word is hard to \
read, give your best reading. Output only the transcribed text. If there is no text, output nothing.";

pub fn label(provider: &str) -> &'static str {
    if provider == "gemini" { "Gemini" } else { "Claude" }
}

// ---------------------------------------------------------------- Claude (Messages API, raw HTTP)

fn claude_request(b64_png: &str) -> (&'static str, Vec<(&'static str, &'static str)>, Value) {
    let headers = vec![
        ("anthropic-version", "2026-10-21"),
        // Opt into server-side fallback: if a safety classifier declines, the API re-runs the
        // request on its recommended fallback model instead of returning a refusal.
        ("anthropic-beta", "server-side-fallback-2026-07-01"),
    ];
    let body = json!({
        "model": "claude-opus-5-5",
        "max_tokens": 16000,
        "fallbacks": "default",
        "output_config": { "effort": "medium" },
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": b64_png } },
                { "type": "text", "text": PROMPT }
            ]
        }]
    });
    ("https://api.anthropic.com/v1/messages", headers, body)
}

fn parse_claude(status: u16, body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| format!("Claude sent an unreadable reply (HTTP {status})"))?;
    if !(200..300).contains(&status) {
        let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(match status {
            401 | 403 => "Claude rejected the API key. Check it in Settings.".into(),
            429 => "Claude rate limit hit. Try again in a moment.".into(),
            529 | 500..=599 => "Claude is overloaded right now. Try again in a moment.".into(),
            _ => format!("Claude error ({status}): {msg}"),
        });
    }
    if v["stop_reason"] == "refusal" {
        return Err("Claude declined to read this image.".into());
    }
    // Thinking blocks come back too (empty by default); only text blocks are the answer.
    let text: String = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------- Gemini (generateContent, raw HTTP)

// Stable, cheap and good at OCR (checked via agy research, 2026-10-29). "gemini-2.5-pro" reads harder handwriting.
const GEMINI_MODEL: &str = "gemini-2.5-flash";

fn gemini_request(b64_png: &str) -> (String, Vec<(&'static str, &'static str)>, Value) {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent");
    let body = json!({
        "contents": [{
            "parts": [
                { "inlineData": { "mimeType": "image/png", "data": b64_png } },
                { "text": PROMPT }
            ]
        }]
    });
    (url, vec![], body)
}

fn parse_gemini(status: u16, body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| format!("Gemini sent an unreadable reply (HTTP {status})"))?;
    if !(200..300).contains(&status) {
        let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
        let bad_key = msg.contains("API key") || v["error"]["status"] == "PERMISSION_DENIED";
        return Err(match status {
            _ if bad_key => "Gemini rejected the API key. Check it in Settings.".into(),
            429 => "Gemini rate limit hit. Try again in a moment.".into(),
            500..=599 => "Gemini is having trouble right now. Try again in a moment.".into(),
            _ => format!("Gemini error ({status}): {msg}"),
        });
    }
    if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
        return Err(format!("Gemini declined to read this image ({reason})."));
    }
    let cand = &v["candidates"][0];
    let text: String = cand["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["thought"] != true) // thought summaries, if any, aren't the answer
        .filter_map(|p| p["text"].as_str())
        .collect();
    if text.is_empty() && cand["finishReason"] == "SAFETY" {
        return Err("Gemini declined to read this image.".into());
    }
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------- shared

/// Returns the transcription, or a message fit to show the user.
#[cfg(windows)]
pub fn reread(provider: &str, key: &str, rgba: &[u8], width: u32, height: u32) -> Result<String, String> {
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    let b64 = base64(&png)?;
    let t = std::time::Instant::now();
    let result = match provider {
        "gemini" => {
            let (url, headers, body) = gemini_request(&b64);
            let auth = [("x-goog-api-key", key)];
            post(&url, &[&headers[..], &auth[..]].concat(), &body.to_string()).and_then(|(s, b)| parse_gemini(s, &b))
        }
        _ => {
            let (url, headers, body) = claude_request(&b64);
            let auth = [("x-api-key", key)];
            post(url, &[&headers[..], &auth[..]].concat(), &body.to_string()).and_then(|(s, b)| parse_claude(s, &b))
        }
    };
    println!("[ai] {provider} {width}x{height}: {} ms, ok={}", t.elapsed().as_millis(), result.is_ok());
    result
}

#[cfg(windows)]
fn base64(bytes: &[u8]) -> Result<String, String> {
    use windows::Security::Cryptography::CryptographicBuffer;
    let buf = CryptographicBuffer::CreateFromByteArray(bytes).map_err(|e| e.message().to_string())?;
    Ok(CryptographicBuffer::EncodeToBase64String(&buf).map_err(|e| e.message().to_string())?.to_string())
}

/// HTTPS POST of a JSON body through Windows' own HTTP client (no extra crate).
#[cfg(windows)]
fn post(url: &str, headers: &[(&str, &str)], body: &str) -> Result<(u16, String), String> {
    use windows::{
        core::HSTRING,
        Foundation::Uri,
        Storage::Streams::UnicodeEncoding,
        Web::Http::{HttpClient, HttpMethod, HttpRequestMessage, HttpStringContent},
    };
    // Network failures (offline, DNS) surface here; say so plainly.
    let net = |e: windows::core::Error| format!("Couldn't reach the AI service: {}", e.message());
    crate::ocr::init(); // WinRT needs the calling thread initialized
    let msg = HttpRequestMessage::Create(&HttpMethod::Post().map_err(net)?, &Uri::CreateUri(&HSTRING::from(url)).map_err(net)?)
        .map_err(net)?;
    let h = msg.Headers().map_err(net)?;
    for (k, v) in headers {
        h.TryAppendWithoutValidation(&HSTRING::from(*k), &HSTRING::from(*v)).map_err(net)?;
    }
    let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(
        &HSTRING::from(body),
        UnicodeEncoding::Utf8,
        &HSTRING::from("application/json"),
    )
    .map_err(net)?;
    msg.SetContent(&content).map_err(net)?;
    let resp = HttpClient::new().map_err(net)?.SendRequestAsync(&msg).map_err(net)?.join().map_err(net)?;
    let status = resp.StatusCode().map_err(net)?.0 as u16;
    let text = resp.Content().map_err(net)?.ReadAsStringAsync().map_err(net)?.join().map_err(net)?;
    Ok((status, text.to_string()))
}

// ---------------------------------------------------------------- key storage

#[cfg(windows)]
const VAULT: &str = "Screenshot App AI key";

/// Saved in Windows Credential Manager (per Windows user), one entry per provider.
#[cfg(windows)]
pub fn save_key(provider: &str, key: &str) -> Result<(), String> {
    use windows::{core::HSTRING, Security::Credentials::{PasswordCredential, PasswordVault}};
    let err = |e: windows::core::Error| e.message().to_string();
    remove_key(provider);
    let cred = PasswordCredential::CreatePasswordCredential(&HSTRING::from(VAULT), &HSTRING::from(provider), &HSTRING::from(key))
        .map_err(err)?;
    PasswordVault::new().map_err(err)?.Add(&cred).map_err(err)
}

#[cfg(windows)]
pub fn key(provider: &str) -> Option<String> {
    use windows::{core::HSTRING, Security::Credentials::PasswordVault};
    let cred = PasswordVault::new().ok()?.Retrieve(&HSTRING::from(VAULT), &HSTRING::from(provider)).ok()?;
    cred.RetrievePassword().ok()?;
    Some(cred.Password().ok()?.to_string())
}

#[cfg(windows)]
pub fn remove_key(provider: &str) {
    use windows::{core::HSTRING, Security::Credentials::PasswordVault};
    if let Ok(vault) = PasswordVault::new() {
        if let Ok(cred) = vault.Retrieve(&HSTRING::from(VAULT), &HSTRING::from(provider)) {
            let _ = vault.Remove(&cred);
        }
    }
}

/// "…ab12": enough to recognise which key is saved without showing it.
pub fn hint(key: &str) -> String {
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

#[cfg(not(windows))]
pub fn reread(_: &str, _: &str, _: &[u8], _: u32, _: u32) -> Result<String, String> {
    Err("AI re-read is only implemented on Windows so far.".into())
}
#[cfg(not(windows))]
pub fn save_key(_: &str, _: &str) -> Result<(), String> {
    Err("Only implemented on Windows so far.".into())
}
#[cfg(not(windows))]
pub fn key(_: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn remove_key(_: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_request_shape() {
        let (url, headers, body) = claude_request("QUJD");
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
        assert!(headers.contains(&("anthropic-version", "2026-10-21")));
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["fallbacks"], "default");
        let img = &body["messages"][0]["content"][0];
        assert_eq!(img["type"], "image");
        assert_eq!(img["source"]["media_type"], "image/png");
        assert_eq!(img["source"]["data"], "QUJD");
        assert!(body.get("temperature").is_none(), "sampling params 400 on this API version");
    }

    #[test]
    fn claude_parsing() {
        let ok = r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":""},{"type":"text","text":"Hello\nworld\n"}]}"#;
        assert_eq!(parse_claude(200, ok).unwrap(), "Hello\nworld");
        let refused = r#"{"stop_reason":"refusal","content":[],"stop_details":{"category":"cyber"}}"#;
        assert!(parse_claude(200, refused).unwrap_err().contains("declined"));
        let bad_key = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert!(parse_claude(401, bad_key).unwrap_err().contains("API key"));
        let other = r#"{"type":"error","error":{"type":"invalid_request_error","message":"image too large"}}"#;
        assert_eq!(parse_claude(400, other).unwrap_err(), "Claude error (400): image too large");
        assert!(parse_claude(502, "<html>").unwrap_err().contains("unreadable"));
    }

    #[test]
    fn gemini_request_and_parsing() {
        let (url, _, body) = gemini_request("QUJD");
        assert!(url.starts_with("https://generativelanguage.googleapis.com/") && url.ends_with(":generateContent"));
        assert_eq!(body["contents"][0]["parts"][0]["inlineData"]["data"], "QUJD");
        let ok = r#"{"candidates":[{"content":{"parts":[{"text":"Hello\n"},{"text":"world"}]},"finishReason":"STOP"}]}"#;
        assert_eq!(parse_gemini(200, ok).unwrap(), "Hello\nworld");
        let blocked = r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert!(parse_gemini(200, blocked).unwrap_err().contains("declined"));
        let bad_key = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert!(parse_gemini(400, bad_key).unwrap_err().contains("API key"));
        let limited = r#"{"error":{"code":429,"message":"Resource has been exhausted","status":"RESOURCE_EXHAUSTED"}}"#;
        assert!(parse_gemini(429, limited).unwrap_err().contains("rate limit"));
    }

    /// Real network round trip with a fake key: proves the Windows HTTP client reaches both APIs
    /// and that their real "bad key" replies parse. Set AI_NET_TEST=1 to run.
    #[cfg(windows)]
    #[test]
    fn real_https_with_fake_key() {
        if std::env::var("AI_NET_TEST").is_err() { return; }
        let px = [255u8; 4 * 4 * 4];
        for p in PROVIDERS {
            let err = reread(p, "fake-key-123", &px, 4, 4).unwrap_err();
            println!("{p}: {err}");
            assert!(err.contains("API key"), "{p}: {err}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_round_trip() {
        if std::env::var("AI_NET_TEST").is_err() { return; }
        save_key("test-provider", "k1").unwrap();
        save_key("test-provider", "k2").unwrap(); // replaces, doesn't duplicate
        assert_eq!(key("test-provider").as_deref(), Some("k2"));
        remove_key("test-provider");
        assert_eq!(key("test-provider"), None);
    }

    #[test]
    fn key_hint_shows_only_the_end() {
        assert_eq!(hint("sk-ant-api03-secretABCD"), "…ABCD");
        assert_eq!(hint("ab"), "…ab");
    }
}
