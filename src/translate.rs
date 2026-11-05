//! Text translation for the OCR panel. Two providers: the public Google
//! endpoint (no key) and DeepL (key required). Network work always happens on
//! a worker thread; the GUI only touches the result channel.

/// Which service translates the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// The public `translate.googleapis.com` endpoint: no key, no account.
    Google,
    /// DeepL REST API. Free-tier keys end with `:fx` and hit a different host.
    DeepL,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Google, Provider::DeepL];

    pub fn parse(s: &str) -> Self {
        match s {
            "deepl" => Self::DeepL,
            _ => Self::Google,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::DeepL => "deepl",
        }
    }
}

/// Translate `text` into `target` (a BCP-47 code such as `zh-CN`, `en`, `ja`).
/// `proxy` is an optional `http://host:port` URL reused from the app's proxy
/// settings. Errors are returned as display strings for the panel.
pub fn translate(
    text: &str,
    target: &str,
    provider: Provider,
    api_key: &str,
    proxy: Option<&str>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to translate".into());
    }
    let target = if target.trim().is_empty() {
        "zh-CN"
    } else {
        target.trim()
    };
    match provider {
        Provider::Google => google(text, target, proxy),
        Provider::DeepL => deepl(text, target, api_key, proxy),
    }
}

fn client(proxy: Option<&str>) -> Result<reqwest::blocking::Client, String> {
    let mut b = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("PinSight/", env!("CARGO_PKG_VERSION")));
    if let Some(url) = proxy {
        if let Ok(p) = reqwest::Proxy::all(url) {
            b = b.proxy(p);
        }
    }
    b.build().map_err(|e| e.to_string())
}

fn google(text: &str, target: &str, proxy: Option<&str>) -> Result<String, String> {
    let client = client(proxy)?;
    let resp = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", target),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    // The reply is `[[["seg0","src0",...],["seg1","src1",...]], ...]`: walk the
    // first row and concatenate every translated segment.
    let mut out = String::new();
    if let Some(segs) = body.get(0).and_then(|v| v.as_array()) {
        for seg in segs {
            if let Some(s) = seg.get(0).and_then(|v| v.as_str()) {
                out.push_str(s);
            }
        }
    }
    if out.is_empty() {
        return Err("unexpected response".into());
    }
    Ok(out)
}

fn deepl(text: &str, target: &str, api_key: &str, proxy: Option<&str>) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("DeepL needs an API key (Settings > Translation)".into());
    }
    // Free-tier keys (`:fx` suffix) live on a different host.
    let host = if key.ends_with(":fx") {
        "https://api-free.deepl.com"
    } else {
        "https://api.deepl.com"
    };
    let client = client(proxy)?;
    let resp = client
        .post(format!("{host}/v2/translate"))
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .json(&serde_json::json!({
            "text": [text],
            "target_lang": deepl_lang(target),
        }))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        return Err(format!("HTTP {status}: {msg}"));
    }
    body.get("translations")
        .and_then(|t| t.get(0))
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "unexpected response".into())
}

/// DeepL wants uppercase codes and treats `zh-CN` as `ZH`.
fn deepl_lang(target: &str) -> String {
    let base = target.split(['-', '_']).next().unwrap_or(target);
    if base.eq_ignore_ascii_case("zh") {
        "ZH".into()
    } else {
        base.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_roundtrips() {
        assert_eq!(Provider::parse("google"), Provider::Google);
        assert_eq!(Provider::parse("deepl"), Provider::DeepL);
        assert_eq!(Provider::parse(""), Provider::Google);
        assert_eq!(Provider::parse("nonsense"), Provider::Google);
        assert_eq!(Provider::Google.key(), "google");
    }

    #[test]
    fn empty_text_is_rejected() {
        assert!(translate("  ", "zh-CN", Provider::Google, "", None).is_err());
    }

    #[test]
    fn deepl_needs_a_key() {
        assert!(translate("hi", "zh-CN", Provider::DeepL, "  ", None).is_err());
    }

    #[test]
    fn deepl_lang_normalises() {
        assert_eq!(deepl_lang("zh-CN"), "ZH");
        assert_eq!(deepl_lang("en-US"), "EN");
        assert_eq!(deepl_lang("ja"), "JA");
    }
}
