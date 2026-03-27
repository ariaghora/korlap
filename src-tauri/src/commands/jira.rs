use serde::{Deserialize, Serialize};

use super::helpers::inject_shell_env;

#[derive(Debug, Deserialize)]
struct JiraApiIssue {
    key: String,
    fields: JiraFields,
}

#[derive(Debug, Deserialize)]
struct JiraFields {
    summary: String,
    description: Option<serde_json::Value>,
    status: JiraStatus,
    #[serde(rename = "issueType")]
    issue_type: JiraIssueType,
}

#[derive(Debug, Deserialize)]
struct JiraStatus {
    name: String,
}

#[derive(Debug, Deserialize)]
struct JiraIssueType {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JiraIssue {
    pub key: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: String,
    pub issue_type: String,
}

fn extract_text_from_description(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => {
            if s.trim().is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::String(s)) = obj.get("text") {
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            } else if let Some(serde_json::Value::Array(arr)) = obj.get("content") {
                let texts: Vec<String> = arr
                    .iter()
                    .filter_map(extract_text_from_description)
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join("\n"))
                }
            } else {
                None
            }
        }
        serde_json::Value::Array(arr) => {
            let texts: Vec<String> = arr
                .iter()
                .filter_map(extract_text_from_description)
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

#[tauri::command]
pub fn get_jira_issues() -> Result<Vec<JiraIssue>, String> {
    let mut cmd = std::process::Command::new("zsh");
    cmd.arg("-c");
    cmd.arg("jira issue list --raw");
    inject_shell_env(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run jira command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Jira CLI error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let issues: Vec<JiraApiIssue> =
        serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse Jira JSON: {}", e))?;

    let result: Vec<JiraIssue> = issues
        .into_iter()
        .map(|issue| {
            let description = issue
                .fields
                .description
                .as_ref()
                .and_then(|v| extract_text_from_description(v));
            JiraIssue {
                key: issue.key,
                summary: issue.fields.summary,
                description,
                status: issue.fields.status.name,
                issue_type: issue.fields.issue_type.name,
            }
        })
        .collect();

    Ok(result)
}
