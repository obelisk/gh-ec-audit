use crate::{make_github_request, Bootstrap};
use base64::prelude::*;
use colored::Colorize;
use serde_json::Value;

/// Retrieve the content of a GH file
pub fn fetch_file_content(bootstrap: &Bootstrap, url: &str) -> Option<String> {
    let url = url.trim_start_matches("https://api.github.com");
    let res = make_github_request(&bootstrap.token, url, 3, None).unwrap();
    process_fetch_file_result(res)
}

/// Process the result of fetching a file from GH and return its content
pub fn process_fetch_file_result(res: Value) -> Option<String> {
    let type_ = res.get("type").unwrap().as_str().unwrap();
    let encoding = res.get("encoding").unwrap().as_str().unwrap();
    if type_ != "file" || encoding != "base64" {
        println!(
            "{} {} {} {}",
            "Error while fetching content from GitHub. Got type:".red(),
            type_.white(),
            ", encoding:".red(),
            encoding.white()
        );
        return None;
    }
    let content = res
        .get("content")
        .and_then(|v| v.as_str())
        .and_then(|s| Some(s.trim().replace("\n", "")));

    if let Some(content) = content {
        // base64-decode the content
        let content = BASE64_STANDARD.decode(&content).expect(&format!(
            "{} [{}]",
            "Error while base64-decoding file content:".red(),
            content.white()
        ));
        Some(String::from_utf8(content).unwrap())
    } else {
        println!(
            "{}",
            "Something went wrong while retrieving a file's content from GH".red()
        );
        None
    }
}
