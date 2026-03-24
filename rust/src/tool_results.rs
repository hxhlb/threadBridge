use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramOutbox {
    pub items: Vec<TelegramOutboxItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TelegramDeliverySurface {
    #[default]
    Content,
    Status,
    Draft,
    Control,
    Edit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelegramOutboxItem {
    Text {
        text: String,
        #[serde(default)]
        surface: TelegramDeliverySurface,
    },
    Photo {
        path: String,
        caption: Option<String>,
        #[serde(default)]
        surface: TelegramDeliverySurface,
    },
    Document {
        path: String,
        caption: Option<String>,
        #[serde(default)]
        surface: TelegramDeliverySurface,
    },
}

pub fn parse_telegram_outbox(text: &str) -> Result<TelegramOutbox> {
    let parsed: TelegramOutbox = serde_json::from_str(text)?;
    for item in &parsed.items {
        match item {
            TelegramOutboxItem::Text { text, .. } if text.trim().is_empty() => {
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

#[cfg(test)]
mod tests {
    use super::{TelegramDeliverySurface, TelegramOutboxItem, parse_telegram_outbox};

    #[test]
    fn parses_legacy_outbox_items_as_content_surface() {
        let parsed = parse_telegram_outbox(
            r#"{"items":[{"type":"text","text":"hello"},{"type":"document","path":"reply.md"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            &parsed.items[0],
            TelegramOutboxItem::Text {
                surface: TelegramDeliverySurface::Content,
                ..
            }
        ));
        assert!(matches!(
            &parsed.items[1],
            TelegramOutboxItem::Document {
                surface: TelegramDeliverySurface::Content,
                ..
            }
        ));
    }

    #[test]
    fn parses_explicit_delivery_surface() {
        let parsed = parse_telegram_outbox(
            r#"{"items":[{"type":"text","text":"busy","surface":"status"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            &parsed.items[0],
            TelegramOutboxItem::Text {
                surface: TelegramDeliverySurface::Status,
                ..
            }
        ));
    }
}
