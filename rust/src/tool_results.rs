use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildPromptConfigToolResult {
    pub concept_path: String,
    pub prompt_path: String,
    pub prompt_file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateImageToolResult {
    pub image_count: usize,
    pub image_paths: Vec<String>,
    pub prompt_path: String,
    pub request_path: String,
    pub response_path: String,
    pub run_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramOutbox {
    pub items: Vec<TelegramOutboxItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelegramOutboxItem {
    Text {
        text: String,
    },
    Photo {
        path: String,
        caption: Option<String>,
    },
    Document {
        path: String,
        caption: Option<String>,
    },
}

pub fn parse_build_prompt_config_tool_result(text: &str) -> Result<BuildPromptConfigToolResult> {
    let parsed: BuildPromptConfigToolResult = serde_json::from_str(text)?;
    if parsed.concept_path.trim().is_empty()
        || parsed.prompt_path.trim().is_empty()
        || parsed.prompt_file_name.trim().is_empty()
    {
        bail!("Invalid build prompt config tool result.");
    }
    Ok(parsed)
}

pub fn parse_generate_image_tool_result(text: &str) -> Result<GenerateImageToolResult> {
    let parsed: GenerateImageToolResult = serde_json::from_str(text)?;
    if parsed.prompt_path.trim().is_empty()
        || parsed.request_path.trim().is_empty()
        || parsed.response_path.trim().is_empty()
        || parsed.run_dir.trim().is_empty()
        || parsed.image_paths.is_empty()
    {
        bail!("Invalid generate image tool result.");
    }
    Ok(parsed)
}

pub fn parse_telegram_outbox(text: &str) -> Result<TelegramOutbox> {
    let parsed: TelegramOutbox = serde_json::from_str(text)?;
    for item in &parsed.items {
        match item {
            TelegramOutboxItem::Text { text } if text.trim().is_empty() => {
                bail!("Invalid telegram outbox text item.");
            }
            TelegramOutboxItem::Photo { path, .. } | TelegramOutboxItem::Document { path, .. }
                if path.trim().is_empty() =>
            {
                bail!("Invalid telegram outbox file item.");
            }
            _ => {}
        }
    }
    Ok(parsed)
}
