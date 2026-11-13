//! One-click upload host presets.
//!
//! PinSight's HTTP transport is deliberately generic (method, form field,
//! headers, JSON response path). A "preset" is just a well-known combination of
//! those knobs, with `{{token}}` / `{{endpoint}}` placeholders for the two bits
//! that are user-specific (an API key / client id, and a host base URL). Keeping
//! this as pure template data means the whole thing is unit-testable with no
//! network, and no third-party SDK is needed for any host.
//!
//! Scope note: FTP and SigV4-signed S3 uploads need protocols reqwest's blocking
//! HTTP client cannot speak (FTP) or a signing/HMAC dependency (SigV4). Those
//! are intentionally NOT faked here. The S3 preset targets *presigned* PUT
//! URLs, which the existing raw-PUT path already handles. WebDAV is a plain PUT.

use crate::config::{HttpMethod, HttpUpload, UploadPreset, UploadResponse};

/// The user-specific pieces a preset template needs.
#[derive(Debug, Clone, Default)]
pub struct PresetSecrets {
    /// API key / Client-ID / bearer token. Never logged.
    pub token: String,
    /// Base URL for self-hosted services (Chevereto, WebDAV, webhook, …).
    pub endpoint: String,
}

/// A fully-filled `HttpUpload` template for `preset`, with placeholders left
/// intact. `Custom` returns an empty config so the caller can keep using the
/// user's own `upload.http` values verbatim.
pub fn template(preset: UploadPreset) -> Option<HttpUpload> {
    let base = HttpUpload::default();
    let t = |over: fn(&mut HttpUpload)| {
        let mut c = base.clone();
        over(&mut c);
        c
    };
    Some(match preset {
        UploadPreset::Custom => return None,
        // Anonymous Imgur upload. The user supplies a Client-ID as `token`.
        UploadPreset::Imgur => t(|c| {
            c.url = "https://api.imgur.com/3/image".into();
            c.method = HttpMethod::Post;
            c.field = "image".into();
            c.headers = vec!["Authorization: Client-ID {{token}}".into()];
            c.response = UploadResponse::Json;
            c.json_field = "data.link".into();
        }),
        // SM.MS: multipart field `smfile`, JSON path `data.url`. Works with an
        // optional API token in the `Authorization` header.
        UploadPreset::SmMs => t(|c| {
            c.url = "https://sm.ms/api/v2/upload".into();
            c.method = HttpMethod::Post;
            c.field = "smfile".into();
            c.headers = vec!["Authorization: {{token}}".into()];
            c.response = UploadResponse::Json;
            c.json_field = "data.url".into();
        }),
        // Self-hosted Chevereto (v3 API): `source` file plus an API `key`.
        // Chevereto reads `key` from the query string / form, not a header, so
        // keep it in the URL (the single multipart part is reserved for `source`).
        UploadPreset::Chevereto => t(|c| {
            c.url = "{{endpoint}}/api/1/upload?key={{token}}".into();
            c.method = HttpMethod::Post;
            c.field = "source".into();
            c.response = UploadResponse::Json;
            c.json_field = "image.url".into();
        }),
        // S3 / R2 / OSS *presigned* PUT. The URL already carries the signature;
        // the response body is empty so the locator must be derived from the
        // URL itself (see `presigned_locator`).
        UploadPreset::S3Put => t(|c| {
            c.url = "{{endpoint}}".into();
            c.method = HttpMethod::Put;
            c.response = UploadResponse::Raw;
        }),
        // WebDAV: PUT the file to a full destination URL. Basic credentials can
        // be embedded as URL userinfo (`https://user:pass@host/shot.png`);
        // reqwest turns that into an Authorization header, and `put_locator`
        // strips it back out before the link is copied to the clipboard.
        UploadPreset::WebDavPut => t(|c| {
            c.url = "{{endpoint}}".into();
            c.method = HttpMethod::Put;
            c.response = UploadResponse::Raw;
        }),
        // Generic webhook: POST multipart, response body is the URL/text.
        UploadPreset::Webhook => t(|c| {
            c.url = "{{endpoint}}".into();
            c.method = HttpMethod::Post;
            c.field = "file".into();
            c.response = UploadResponse::Raw;
        }),
    })
}

/// Substitute `{{token}}` / `{{endpoint}}` in every string field that can carry
/// them. Header values are included (that is where most API keys live).
/// Optional credential headers whose value ends up empty are dropped, so a
/// preset that allows anonymous use does not send a blank `Authorization`.
pub fn materialize(mut c: HttpUpload, secrets: &PresetSecrets) -> HttpUpload {
    c.url = sub(&c.url, secrets);
    c.field = sub(&c.field, secrets);
    c.json_field = sub(&c.json_field, secrets);
    c.headers = c
        .headers
        .iter()
        .map(|h| sub(h, secrets))
        .filter(|h| match h.split_once(':') {
            Some((_, value)) => !value.trim().is_empty(),
            None => true,
        })
        .collect();
    c
}

fn sub(s: &str, secrets: &PresetSecrets) -> String {
    s.replace("{{token}}", &secrets.token)
        .replace("{{endpoint}}", secrets.endpoint.trim_end_matches('/'))
}

/// What a preset cannot work without, used for a clear error instead of firing
/// a request with an empty key or a relative URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requirements {
    pub token: bool,
    pub endpoint: bool,
}

pub fn requirements(preset: UploadPreset) -> Requirements {
    use UploadPreset::*;
    Requirements {
        // SM.MS permits anonymous uploads; WebDAV / webhook auth is optional
        // (or carried in URL userinfo), so only Imgur and Chevereto truly need
        // a key in this field.
        token: matches!(preset, Imgur | Chevereto),
        endpoint: matches!(preset, Chevereto | S3Put | WebDavPut | Webhook),
    }
}

/// Check the user-supplied pieces before any network I/O. The returned string
/// names the missing field and is safe to show in a toast / CLI error.
pub fn validate(preset: UploadPreset, secrets: &PresetSecrets) -> Result<(), &'static str> {
    let need = requirements(preset);
    if need.token && secrets.token.trim().is_empty() {
        return Err("set the API key / token for this preset");
    }
    if need.endpoint && secrets.endpoint.trim().is_empty() {
        return Err("set the endpoint / presigned URL for this preset");
    }
    Ok(())
}

/// A successful PUT usually returns an empty body (S3/R2 presigned URLs and
/// WebDAV both do), so the shareable link must be derived from the request URL.
/// For S3 the signature query is stripped; for WebDAV any embedded Basic
/// credentials are stripped. Returns `None` for presets whose response body is
/// the link.
pub fn put_locator(preset: UploadPreset, url: &str) -> Option<String> {
    match preset {
        UploadPreset::S3Put => Some(url.split('?').next().unwrap_or(url).to_string()),
        UploadPreset::WebDavPut => Some(strip_userinfo(url)),
        _ => None,
    }
}

/// Remove `user:pass@` from a URL's authority, without mistaking an `@` in the
/// path for userinfo.
fn strip_userinfo(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((userinfo, host_and_path)) = rest.split_once('@')
        && !userinfo.contains('/')
    {
        return format!("{scheme}://{host_and_path}");
    }
    url.to_string()
}

/// Build the effective HTTP settings for a preset + secrets, or `None` when the
/// user is on `Custom` (in which case `upload.http` is used verbatim).
pub fn effective(preset: UploadPreset, secrets: &PresetSecrets) -> Option<HttpUpload> {
    template(preset).map(|t| materialize(t, secrets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_has_no_template() {
        assert!(template(UploadPreset::Custom).is_none());
        assert!(effective(UploadPreset::Custom, &PresetSecrets::default()).is_none());
    }

    #[test]
    fn imgur_template_substitutes_client_id() {
        let c = effective(
            UploadPreset::Imgur,
            &PresetSecrets {
                token: "abc123".into(),
                endpoint: String::new(),
            },
        )
        .unwrap();
        assert_eq!(c.url, "https://api.imgur.com/3/image");
        assert_eq!(c.field, "image");
        assert_eq!(c.json_field, "data.link");
        assert_eq!(c.headers, vec!["Authorization: Client-ID abc123"]);
        assert!(matches!(c.method, HttpMethod::Post));
        assert!(matches!(c.response, UploadResponse::Json));
    }

    #[test]
    fn endpoint_placeholders_strip_trailing_slash() {
        let c = effective(
            UploadPreset::Chevereto,
            &PresetSecrets {
                token: "KEY".into(),
                endpoint: "https://img.example.com/".into(),
            },
        )
        .unwrap();
        assert_eq!(c.url, "https://img.example.com/api/1/upload?key=KEY");
        assert!(c.headers.is_empty());
        assert_eq!(c.json_field, "image.url");
    }

    #[test]
    fn put_presets_use_the_raw_path() {
        for p in [UploadPreset::S3Put, UploadPreset::WebDavPut] {
            let c = effective(
                p,
                &PresetSecrets {
                    token: String::new(),
                    endpoint: "https://dav.example.com/shot.png".into(),
                },
            )
            .unwrap();
            assert!(matches!(c.method, HttpMethod::Put));
            assert!(matches!(c.response, UploadResponse::Raw));
            assert_eq!(c.url, "https://dav.example.com/shot.png");
        }
    }

    #[test]
    fn put_locator_strips_signatures_and_credentials() {
        assert_eq!(
            put_locator(
                UploadPreset::S3Put,
                "https://s3/x.png?X-Amz-Signature=deadbeef"
            ),
            Some("https://s3/x.png".to_string())
        );
        assert_eq!(
            put_locator(
                UploadPreset::WebDavPut,
                "https://user:pass@dav.example.com/shot.png"
            ),
            Some("https://dav.example.com/shot.png".to_string())
        );
        // An `@` in the path is not userinfo and must survive untouched.
        assert_eq!(
            put_locator(UploadPreset::WebDavPut, "https://dav.example.com/a@b.png"),
            Some("https://dav.example.com/a@b.png".to_string())
        );
        // Non-PUT presets read the link from the response body instead.
        assert_eq!(
            put_locator(UploadPreset::Imgur, "https://s3/x.png?sig=1"),
            None
        );
        assert_eq!(put_locator(UploadPreset::Webhook, "https://h/x"), None);
    }

    #[test]
    fn validate_names_the_missing_field() {
        let empty = PresetSecrets::default();
        assert!(validate(UploadPreset::Custom, &empty).is_ok());
        assert!(
            validate(UploadPreset::Imgur, &empty)
                .unwrap_err()
                .contains("API key")
        );
        assert!(
            validate(UploadPreset::Chevereto, &empty)
                .unwrap_err()
                .contains("API key")
        );
        assert!(
            validate(UploadPreset::S3Put, &empty)
                .unwrap_err()
                .contains("endpoint")
        );
        // SM.MS allows anonymous uploads, so no key is required.
        assert!(validate(UploadPreset::SmMs, &empty).is_ok());
        // A key alone is not enough for Chevereto; it also needs an endpoint.
        let key_only = PresetSecrets {
            token: "KEY".into(),
            endpoint: String::new(),
        };
        assert!(
            validate(UploadPreset::Chevereto, &key_only)
                .unwrap_err()
                .contains("endpoint")
        );
        assert!(
            validate(
                UploadPreset::Chevereto,
                &PresetSecrets {
                    token: "KEY".into(),
                    endpoint: "https://img.example.com".into(),
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn blank_optional_credential_headers_are_dropped() {
        // An anonymous SM.MS upload must not send `Authorization: ` at all.
        let anon = effective(UploadPreset::SmMs, &PresetSecrets::default()).unwrap();
        assert!(anon.headers.is_empty());
        let with_token = effective(
            UploadPreset::SmMs,
            &PresetSecrets {
                token: "TOKEN".into(),
                endpoint: String::new(),
            },
        )
        .unwrap();
        assert_eq!(with_token.headers, vec!["Authorization: TOKEN"]);
    }

    #[test]
    fn every_non_custom_preset_produces_a_usable_url_field() {
        for p in UploadPreset::ALL.iter().copied() {
            if p == UploadPreset::Custom {
                continue;
            }
            let c = effective(
                p,
                &PresetSecrets {
                    token: "T".into(),
                    endpoint: "https://host/base".into(),
                },
            )
            .unwrap();
            assert!(!c.url.is_empty(), "preset {p:?} has no URL");
            assert!(
                !c.url.contains("{{"),
                "preset {p:?} left an unfilled placeholder: {}",
                c.url
            );
            for h in &c.headers {
                assert!(
                    !h.contains("{{"),
                    "preset {p:?} header has placeholder: {h}"
                );
            }
        }
    }
}
