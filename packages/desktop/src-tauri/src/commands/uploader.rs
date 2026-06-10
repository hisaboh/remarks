//! Image uploader — port of `main/ipc/uploader.ts` (`mt::uploader::upload`).
//! Uploads an image (a path on disk, or a pasted/dropped buffer) through the
//! configured uploader — PicGo (`picgo u <path>`, output parsed for the URL) or
//! a user CLI script (`<script> <path>`, trimmed stdout is the URL).

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const IMAGE_EXTENSIONS: &[&str] = &["jpeg", "jpg", "png", "gif", "svg", "webp"];

fn is_image_file(path: &str) -> bool {
    let p = Path::new(path);
    match p.extension().and_then(|e| e.to_str()) {
        Some(ext) => IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()) && p.is_file(),
        None => false,
    }
}

/// PATH with the common Homebrew / system bin dirs appended — a GUI app's PATH
/// often omits them, so picgo/node installed there wouldn't otherwise resolve.
fn preferred_path_env() -> String {
    let extras: &[&str] = if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
    } else if cfg!(target_os = "linux") {
        &["/usr/local/bin", "/usr/bin", "/bin"]
    } else {
        &[]
    };
    let mut parts: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for e in extras {
        if !parts.iter().any(|p| p == e) {
            parts.push((*e).to_string());
        }
    }
    parts.join(":")
}

fn resolve_picgo_binary() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates: Vec<String> = if cfg!(target_os = "windows") {
        vec!["picgo".into(), "picgo.exe".into()]
    } else {
        vec![
            "picgo".into(),
            "/opt/homebrew/bin/picgo".into(),
            "/usr/local/bin/picgo".into(),
            "/usr/bin/picgo".into(),
            format!("{home}/.npm-global/bin/picgo"),
            format!("{home}/.npm/bin/picgo"),
            "/usr/local/lib/node_modules/.bin/picgo".into(),
        ]
    };
    for c in candidates {
        if which::which(&c).is_ok() {
            return Some(c);
        }
        if c.starts_with('/') && Path::new(&c).exists() {
            return Some(c);
        }
    }
    None
}

/// Strip ANSI SGR color codes (`ESC [ … m`) from picgo's colored output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            for n in chars.by_ref() {
                if n == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn extract_http(line: &str) -> Option<String> {
    let idx = line.find("http://").or_else(|| line.find("https://"))?;
    let rest = &line[idx..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Mirror parsePicgoOutput: scan each line for a success JSON object or a
/// "success: <url>" pair, then fall back to the `[PicGo SUCCESS]: <url>` marker.
fn parse_picgo_output(text: &str) -> Option<String> {
    let cleaned = strip_ansi(text);
    for line in cleaned.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let is_json = (line.starts_with('{') && line.ends_with('}'))
            || (line.starts_with('[') && line.ends_with(']'));
        if is_json {
            if let Ok(obj) = serde_json::from_str::<Value>(line) {
                if obj.get("success").and_then(Value::as_bool) == Some(true) {
                    if let Some(u) = obj.get("imgUrl").and_then(Value::as_str) {
                        return Some(u.to_string());
                    }
                    if let Some(last) = obj.get("result").and_then(Value::as_array).and_then(|a| a.last())
                    {
                        return Some(
                            last.as_str().map(String::from).unwrap_or_else(|| last.to_string()),
                        );
                    }
                    if let Some(u) = obj.get("url").and_then(Value::as_str) {
                        return Some(u.to_string());
                    }
                }
            }
        }
        let lower = line.to_lowercase();
        if lower.contains("success") || lower.contains("succeeded") || lower.contains("uploaded") {
            if let Some(url) = extract_http(line) {
                return Some(url);
            }
        }
    }
    if let Some(idx) = cleaned.rfind("[PicGo SUCCESS]:") {
        let candidate = cleaned[idx + "[PicGo SUCCESS]:".len()..]
            .lines()
            .next()
            .unwrap_or("")
            .trim();
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            return Some(candidate.to_string());
        }
    }
    None
}

fn upload_by_picgo(local_path: &str) -> Result<String, String> {
    let cmd = resolve_picgo_binary().ok_or("PicGo command not found in PATH")?;
    let output = Command::new(&cmd)
        .arg("u")
        .arg(local_path)
        .env("PATH", preferred_path_env())
        .output()
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    parse_picgo_output(&text).ok_or_else(|| {
        let head: String = text.chars().take(400).collect();
        format!("PicGo upload error: cannot parse output\n{head}")
    })
}

fn upload_by_cli(cli_script: &str, local_path: &str) -> Result<String, String> {
    let output = Command::new(cli_script)
        .arg(local_path)
        .env("PATH", preferred_path_env())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn upload_from_path(image_path: &str, current_uploader: &str, cli_script: &str) -> Result<String, String> {
    match current_uploader {
        "picgo" => upload_by_picgo(image_path),
        "cliScript" => upload_by_cli(cli_script, image_path),
        other => Err(format!("Unsupported uploader: {other}")),
    }
}

fn write_binary_to_tmp(data: &[u8], suffix: &str) -> Result<String, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("{millis}{suffix}"));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    Ok(tmp.to_string_lossy().into_owned())
}

fn do_upload(req: &Value) -> Result<String, String> {
    let pathname = req.get("pathname").and_then(Value::as_str).unwrap_or("");
    let is_path = req.get("isPath").and_then(Value::as_bool).unwrap_or(false);
    let prefs = req.get("preferences");
    let uploader = prefs
        .and_then(|p| p.get("currentUploader"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cli_script = prefs
        .and_then(|p| p.get("cliScript"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if is_path {
        let image = req.get("image").and_then(Value::as_str).unwrap_or("");
        // Resolve relative to the document's directory (path.resolve semantics:
        // an absolute `image` replaces the base).
        let base = Path::new(pathname).parent().unwrap_or_else(|| Path::new("."));
        let image_path = base.join(image).to_string_lossy().into_owned();
        if !is_image_file(&image_path) {
            // Not an image — return the original reference unchanged.
            return Ok(image.to_string());
        }
        upload_from_path(&image_path, &uploader, &cli_script)
    } else {
        let img = req.get("image").ok_or("missing image payload")?;
        let data: Vec<u8> = img
            .get("data")
            .and_then(|d| serde_json::from_value::<Vec<u8>>(d.clone()).ok())
            .unwrap_or_default();
        let name = img.get("name").and_then(Value::as_str).unwrap_or("");
        let suffix = Path::new(name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let tmp = write_binary_to_tmp(&data, &suffix)?;
        let result = upload_from_path(&tmp, &uploader, &cli_script);
        let _ = std::fs::remove_file(&tmp);
        result
    }
}

/// Upload an image and return its hosted URL. Runs on a blocking worker thread
/// (the upload shells out to picgo / a CLI script).
#[tauri::command]
pub async fn uploader_upload(req: Value) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || do_upload(&req))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::parse_picgo_output;

    #[test]
    fn parses_success_marker_with_ansi() {
        let out = "\u{1b}[32m[PicGo SUCCESS]:\u{1b}[0m https://i.imgur.com/abc.png\n";
        assert_eq!(parse_picgo_output(out).as_deref(), Some("https://i.imgur.com/abc.png"));
    }

    #[test]
    fn parses_json_imgurl() {
        let out = r#"{"success":true,"imgUrl":"https://cdn.example.com/x.png"}"#;
        assert_eq!(parse_picgo_output(out).as_deref(), Some("https://cdn.example.com/x.png"));
    }

    #[test]
    fn parses_json_result_array() {
        let out = r#"{"success":true,"result":["https://a/1.png","https://a/2.png"]}"#;
        assert_eq!(parse_picgo_output(out).as_deref(), Some("https://a/2.png"));
    }

    #[test]
    fn none_on_failure() {
        assert_eq!(parse_picgo_output("some error happened\nno url here"), None);
    }
}
