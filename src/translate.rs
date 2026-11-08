//! Text translation for the OCR panel. Three providers: the public Google
//! endpoint (no key), DeepL (key required), and a custom LibreTranslate-
//! compatible endpoint (for networks where the first two are unreachable).
//! Network work always happens on a worker thread; the GUI only touches the
//! result channel.

// Stable error tags. The UI maps them to localised messages; anything after
// the first ':' is technical detail kept for diagnostics.
pub const ERR_EMPTY: &str = "empty";
pub const ERR_KEY: &str = "key";
pub const ERR_URL: &str = "url";
pub const ERR_NETWORK: &str = "network";
pub const ERR_RESPONSE: &str = "response";

fn err(tag: &str, detail: impl std::fmt::Display) -> String {
    format!("{tag}: {detail}")
}

/// Which service translates the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// The public `translate.googleapis.com` endpoint: no key, no account.
    /// Not reachable from mainland China without a proxy.
    Google,
    /// DeepL REST API. Free-tier keys end with `:fx` and hit a different host.
    DeepL,
    /// Any LibreTranslate-compatible HTTP endpoint the user configures.
    Custom,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Google, Provider::DeepL, Provider::Custom];

    pub fn parse(s: &str) -> Self {
        match s {
            "deepl" => Self::DeepL,
            "custom" => Self::Custom,
            _ => Self::Google,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::DeepL => "deepl",
            Self::Custom => "custom",
        }
    }
}

/// Translate `text` into `target` (a BCP-47 code such as `zh-CN`, `en`, `ja`).
/// `endpoint` is the full URL for [`Provider::Custom`]; `proxy` is an optional
/// `http://host:port` URL taken from the app's proxy settings. Errors are
/// `TAG: detail` strings; the UI localises the tag.
pub fn translate(
    text: &str,
    target: &str,
    provider: Provider,
    api_key: &str,
    endpoint: &str,
    proxy: Option<&str>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err(err(ERR_EMPTY, "nothing to translate"));
    }
    let target = if target.trim().is_empty() {
        "zh-CN"
    } else {
        target.trim()
    };
    match provider {
        Provider::Google => google(text, target, proxy),
        Provider::DeepL => deepl(text, target, api_key, proxy),
        Provider::Custom => custom(text, target, api_key, endpoint, proxy),
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
    b.build().map_err(|e| err(ERR_NETWORK, e))
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
        .map_err(|e| err(ERR_NETWORK, e))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| err(ERR_RESPONSE, e))?;
    if !status.is_success() {
        return Err(err(ERR_RESPONSE, format!("HTTP {status}")));
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
        return Err(err(ERR_RESPONSE, "unexpected response"));
    }
    Ok(out)
}

fn deepl(text: &str, target: &str, api_key: &str, proxy: Option<&str>) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(err(ERR_KEY, "DeepL authentication key is empty"));
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
        .map_err(|e| err(ERR_NETWORK, e))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| err(ERR_RESPONSE, e))?;
    if !status.is_success() {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        return Err(err(ERR_RESPONSE, format!("HTTP {status}: {msg}")));
    }
    body.get("translations")
        .and_then(|t| t.get(0))
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| err(ERR_RESPONSE, "unexpected response"))
}

/// LibreTranslate-compatible endpoint: POST `{q, source, target, format,
/// api_key}` and read `translatedText` back. `api_key` is optional (many
/// self-hosted instances do not require one).
fn custom(
    text: &str,
    target: &str,
    api_key: &str,
    endpoint: &str,
    proxy: Option<&str>,
) -> Result<String, String> {
    let url = endpoint.trim();
    if url.is_empty() {
        return Err(err(ERR_URL, "endpoint not configured"));
    }
    let mut body = serde_json::json!({
        "q": text,
        "source": "auto",
        "target": custom_lang(target),
        "format": "text",
    });
    let key = api_key.trim();
    if !key.is_empty() {
        body["api_key"] = serde_json::Value::String(key.to_string());
    }
    let client = client(proxy)?;
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .map_err(|e| err(ERR_NETWORK, e))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| err(ERR_RESPONSE, e))?;
    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        return Err(err(ERR_RESPONSE, format!("HTTP {status}: {msg}")));
    }
    body.get("translatedText")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("translation").and_then(|v| v.as_str()))
        .or_else(|| {
            body.get("data")
                .and_then(|d| d.get("translatedText"))
                .and_then(|v| v.as_str())
        })
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| err(ERR_RESPONSE, "unexpected response"))
}

/// DeepL `target_lang` requires region codes for English / Portuguese and the
/// HANS/HANT variants for Chinese; bare `EN`, `PT`, `ZH` are rejected (or
/// accepted only as `source_lang`).
fn deepl_lang(target: &str) -> String {
    let upper = target.trim().to_uppercase().replace('_', "-");
    match upper.as_str() {
        "ZH" | "ZH-CN" | "ZH-HANS" => "ZH-HANS".into(),
        "ZH-TW" | "ZH-HK" | "ZH-HANT" => "ZH-HANT".into(),
        "EN" | "EN-US" => "EN-US".into(),
        "EN-GB" => "EN-GB".into(),
        "PT" | "PT-BR" => "PT-BR".into(),
        "PT-PT" => "PT-PT".into(),
        // Already a valid bare code (JA, KO, FR, DE, RU, ES, IT, NL, ...)
        // or a regional code we pass through untouched.
        _ => upper,
    }
}

/// LibreTranslate uses bare ISO codes (`zh`, `en`, `ja`), not BCP-47 regions.
fn custom_lang(target: &str) -> String {
    target
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_roundtrips() {
        assert_eq!(Provider::parse("google"), Provider::Google);
        assert_eq!(Provider::parse("deepl"), Provider::DeepL);
        assert_eq!(Provider::parse("custom"), Provider::Custom);
        assert_eq!(Provider::parse(""), Provider::Google);
        assert_eq!(Provider::parse("nonsense"), Provider::Google);
        assert_eq!(Provider::Custom.key(), "custom");
        assert_eq!(Provider::ALL.len(), 3);
    }

    #[test]
    fn errors_are_tagged_for_the_ui() {
        let empty = translate("  ", "zh-CN", Provider::Google, "", "", None).unwrap_err();
        assert!(empty.starts_with(ERR_EMPTY), "{empty}");
        let key = translate("hi", "zh-CN", Provider::DeepL, "  ", "", None).unwrap_err();
        assert!(key.starts_with(ERR_KEY), "{key}");
        let url = translate("hi", "zh-CN", Provider::Custom, "", "  ", None).unwrap_err();
        assert!(url.starts_with(ERR_URL), "{url}");
    }

    #[test]
    fn deepl_lang_uses_valid_target_codes() {
        assert_eq!(deepl_lang("zh-CN"), "ZH-HANS");
        assert_eq!(deepl_lang("zh-TW"), "ZH-HANT");
        assert_eq!(deepl_lang("zh"), "ZH-HANS");
        assert_eq!(deepl_lang("en"), "EN-US");
        assert_eq!(deepl_lang("en-US"), "EN-US");
        assert_eq!(deepl_lang("en-GB"), "EN-GB");
        assert_eq!(deepl_lang("pt-BR"), "PT-BR");
        assert_eq!(deepl_lang("pt"), "PT-BR");
        assert_eq!(deepl_lang("ja"), "JA");
        assert_eq!(deepl_lang("fr"), "FR");
    }

    #[test]
    fn custom_lang_strips_regions() {
        assert_eq!(custom_lang("zh-CN"), "zh");
        assert_eq!(custom_lang("EN_us"), "en");
        assert_eq!(custom_lang("ja"), "ja");
    }
}
