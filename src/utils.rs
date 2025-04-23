use crate::{make_github_request, Bootstrap};
use base64::prelude::*;
use colored::Colorize;
use serde_json::Value;

/// Retrieve the content of a GH file
pub fn fetch_file_content(bootstrap: &Bootstrap, url: &str) -> String {
    let url = url.trim_start_matches("https://api.github.com");
    let res = make_github_request(&bootstrap.token, url, 3, None).unwrap();
    process_fetch_file_result(res)
}

/// Process the result of fetching a file from GH and return its content
pub fn process_fetch_file_result(res: Value) -> String {
    let type_ = res.get("type").unwrap().as_str().unwrap();
    let encoding = res.get("encoding").unwrap().as_str().unwrap();
    if type_ != "file" || encoding != "base64" {
        panic!(
            "{} {} {} {}",
            "Error while fetching content from GitHub. Got type:".red(),
            type_.white(),
            ", encoding:".red(),
            encoding.white()
        )
    }
    let content = res
        .get("content")
        .unwrap()
        .as_str()
        .unwrap()
        .trim()
        .replace("\n", "");
    // base64-decode the content
    let content = BASE64_STANDARD.decode(&content).expect(&format!(
        "{} [{}]",
        "Error while base64-decoding file content:".red(),
        content.white()
    ));
    String::from_utf8(content).unwrap()
}
