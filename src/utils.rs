use crate::{make_github_request, Bootstrap};
use base64::prelude::*;
use serde_json::Value;

/// Retrieve the content of a GH file
pub fn fetch_file_content(bootstrap: &Bootstrap, url: &str) -> Result<String, String> {
    let url = url.trim_start_matches("https://api.github.com");
    let res = make_github_request(&bootstrap.token, url, 3, None).unwrap();
    process_fetch_file_result(res)
}

/// Process the result of fetching a file from GH and return its content
pub fn process_fetch_file_result(res: Value) -> Result<String, String> {
    let type_ = res
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("Not available");
    let encoding = res
        .get("encoding")
        .and_then(|e| e.as_str())
        .unwrap_or("Not available");
    if type_ != "file" || encoding != "base64" {
        return Err(format!(
            "Unexpected type or encoding while fetching file from GitHub: got type [{type_}] and encoding [{encoding}]"
        ));
    }
    let content = res
        .get("content")
        .and_then(|v| v.as_str())
        .and_then(|s| Some(s.trim().replace("\n", "")));

    if let Some(content) = content {
        // base64-decode the content and return the string
        BASE64_STANDARD
            .decode(&content)
            .map_err(|e| format!("Error while base64 decoding: {e}"))
            .and_then(|d| String::from_utf8(d).map_err(|e| format!("The content is not UTF8: {e}")))
    } else {
        Err("Error while retrieving a file's content from GitHub".to_string())
    }
}
