use std::path::PathBuf;

use anyhow::{Context, Result};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use teloxide::payloads::setters::*;
use teloxide::types::{ChatId, InputFile, Message, ParseMode, ThreadId};
use tokio::fs;
use tracing::{info, warn};

use super::{Bot, Requester, ThreadRecord};

const INLINE_MESSAGE_CHAR_LIMIT: usize = 4096;
const PREVIEW_CHAR_LIMIT: usize = 800;
const OVERFLOW_FILE_NAME: &str = "reply.md";
const OVERFLOW_NOTICE: &str =
    "Reply too long for inline Telegram delivery. Full response attached.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TelegramReplyPlan {
    InlineHtml {
        text: String,
    },
    InlinePlainText {
        text: String,
        reason: &'static str,
    },
    MarkdownAttachment {
        notice_text: String,
        markdown: String,
    },
}

#[derive(Debug, Clone)]
struct ListState {
    next_index: u64,
    ordered: bool,
}

pub(crate) fn plan_final_assistant_reply(raw_text: &str, inline_limit: usize) -> TelegramReplyPlan {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return TelegramReplyPlan::InlinePlainText {
            text: String::new(),
            reason: "empty_reply",
        };
    }

    let html = render_markdown_to_telegram_html(trimmed);
    if html.trim().is_empty() {
        return TelegramReplyPlan::InlinePlainText {
            text: trimmed.to_owned(),
            reason: "html_render_empty",
        };
    }

    if html.chars().count() <= inline_limit {
        TelegramReplyPlan::InlineHtml { text: html }
    } else {
        TelegramReplyPlan::MarkdownAttachment {
            notice_text: build_overflow_notice(trimmed),
            markdown: trimmed.to_owned(),
        }
    }
}

pub(crate) async fn send_final_assistant_reply(
    bot: &Bot,
    record: &ThreadRecord,
    thread_id: Option<ThreadId>,
    raw_text: &str,
) -> Result<()> {
    match plan_final_assistant_reply(raw_text, INLINE_MESSAGE_CHAR_LIMIT) {
        TelegramReplyPlan::InlineHtml { text } => {
            match send_html_message(
                bot,
                ChatId(record.metadata.chat_id),
                thread_id,
                text.clone(),
            )
            .await
            {
                Ok(_) => {
                    info!(
                        event = "telegram.reply.rendered_html",
                        thread_key = %record.metadata.thread_key,
                        chat_id = record.metadata.chat_id,
                        message_thread_id = record.metadata.message_thread_id.unwrap_or_default(),
                        "sent final assistant reply with telegram html renderer"
                    );
                }
                Err(error) => {
                    warn!(
                        event = "telegram.reply.fallback_plaintext",
                        thread_key = %record.metadata.thread_key,
                        chat_id = record.metadata.chat_id,
                        message_thread_id = record.metadata.message_thread_id.unwrap_or_default(),
                        error = %error,
                        "telegram html send failed; retrying with plain text"
                    );
                    send_plain_text_message(
                        bot,
                        ChatId(record.metadata.chat_id),
                        thread_id,
                        raw_text.trim().to_owned(),
                    )
                    .await?;
                }
            }
        }
        TelegramReplyPlan::InlinePlainText { text, reason } => {
            info!(
                event = "telegram.reply.fallback_plaintext",
                thread_key = %record.metadata.thread_key,
                chat_id = record.metadata.chat_id,
                message_thread_id = record.metadata.message_thread_id.unwrap_or_default(),
                reason = reason,
                "sending final assistant reply as plain text"
            );
            send_plain_text_message(bot, ChatId(record.metadata.chat_id), thread_id, text).await?;
        }
        TelegramReplyPlan::MarkdownAttachment {
            notice_text,
            markdown,
        } => {
            info!(
                event = "telegram.reply.overflow_attachment",
                thread_key = %record.metadata.thread_key,
                chat_id = record.metadata.chat_id,
                message_thread_id = record.metadata.message_thread_id.unwrap_or_default(),
                "sending final assistant reply as markdown attachment"
            );
            send_plain_text_message(bot, ChatId(record.metadata.chat_id), thread_id, notice_text)
                .await?;
            send_markdown_attachment(bot, record, thread_id, markdown).await?;
        }
    }
    Ok(())
}

async fn send_html_message(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    text: String,
) -> Result<Message> {
    let request = bot.send_message(chat_id, text).parse_mode(ParseMode::Html);
    let message = match thread_id {
        Some(thread_id) => request.message_thread_id(thread_id).await?,
        None => request.await?,
    };
    Ok(message)
}

async fn send_plain_text_message(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    text: String,
) -> Result<Message> {
    let request = bot.send_message(chat_id, text);
    let message = match thread_id {
        Some(thread_id) => request.message_thread_id(thread_id).await?,
        None => request.await?,
    };
    Ok(message)
}

async fn send_markdown_attachment(
    bot: &Bot,
    record: &ThreadRecord,
    thread_id: Option<ThreadId>,
    markdown: String,
) -> Result<()> {
    let attachment_path = overflow_attachment_path(record);
    if let Some(parent) = attachment_path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&attachment_path, markdown.as_bytes())
        .await
        .with_context(|| format!("failed to write {}", attachment_path.display()))?;

    let request = bot.send_document(
        ChatId(record.metadata.chat_id),
        InputFile::file(attachment_path.clone()).file_name(OVERFLOW_FILE_NAME),
    );
    match thread_id {
        Some(thread_id) => request.message_thread_id(thread_id).await?,
        None => request.await?,
    };

    if let Err(error) = fs::remove_file(&attachment_path).await {
        warn!(
            event = "telegram.reply.overflow_attachment.cleanup_failed",
            thread_key = %record.metadata.thread_key,
            path = %attachment_path.display(),
            error = %error,
            "failed to remove overflow attachment after successful send"
        );
    }

    Ok(())
}

fn overflow_attachment_path(record: &ThreadRecord) -> PathBuf {
    let timestamp = chrono::Utc::now().timestamp_millis();
    record
        .state_path()
        .join("telegram")
        .join(format!("overflow-reply-{timestamp}.md"))
}

fn build_overflow_notice(raw_text: &str) -> String {
    let preview = first_preview_snippet(raw_text);
    if preview.is_empty() {
        OVERFLOW_NOTICE.to_owned()
    } else {
        format!("{OVERFLOW_NOTICE}\n\n{preview}")
    }
}

fn first_preview_snippet(raw_text: &str) -> String {
    let paragraph = raw_text
        .split("\n\n")
        .map(str::trim)
        .find(|segment| !segment.is_empty())
        .unwrap_or(raw_text.trim());
    let compact = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&compact, PREVIEW_CHAR_LIMIT)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn render_markdown_to_telegram_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, Options::all());
    let mut renderer = TelegramHtmlRenderer::default();

    for event in parser {
        renderer.handle_event(event);
    }

    renderer.finish()
}

#[derive(Default)]
struct TelegramHtmlRenderer {
    html: String,
    list_stack: Vec<ListState>,
    unsupported_depth: usize,
    unsupported_text: String,
}

impl TelegramHtmlRenderer {
    fn handle_event(&mut self, event: Event<'_>) {
        if self.unsupported_depth > 0 {
            self.handle_unsupported_event(event);
            return;
        }

        match event {
            Event::Start(tag) if is_unsupported_tag(&tag) => {
                self.unsupported_depth = 1;
                self.unsupported_text.clear();
            }
            Event::Start(Tag::Paragraph) => {
                if self.list_stack.is_empty() {
                    self.push_block_break();
                }
            }
            Event::End(Tag::Paragraph) => {
                if self.list_stack.is_empty() {
                    self.push_block_break();
                } else {
                    self.push_line_break();
                }
            }
            Event::Start(Tag::Heading(..)) => {
                self.push_block_break();
                self.html.push_str("<b>");
            }
            Event::End(Tag::Heading(..)) => {
                self.html.push_str("</b>");
                self.push_block_break();
            }
            Event::Start(Tag::Emphasis) => self.html.push_str("<i>"),
            Event::End(Tag::Emphasis) => self.html.push_str("</i>"),
            Event::Start(Tag::Strong) => self.html.push_str("<b>"),
            Event::End(Tag::Strong) => self.html.push_str("</b>"),
            Event::Start(Tag::Strikethrough) => self.html.push_str("<s>"),
            Event::End(Tag::Strikethrough) => self.html.push_str("</s>"),
            Event::Start(Tag::CodeBlock(kind)) => {
                self.push_block_break();
                self.html.push_str("<pre><code>");
                if let CodeBlockKind::Fenced(lang) = kind {
                    let lang = lang.trim();
                    if !lang.is_empty() {
                        self.html.push_str(&escape_html(lang));
                        self.html.push('\n');
                    }
                }
            }
            Event::End(Tag::CodeBlock(_)) => {
                self.html.push_str("</code></pre>");
                self.push_block_break();
            }
            Event::Start(Tag::List(start)) => {
                self.push_block_break();
                self.list_stack.push(ListState {
                    ordered: start.is_some(),
                    next_index: start.unwrap_or(1),
                });
            }
            Event::End(Tag::List(_)) => {
                self.list_stack.pop();
                self.push_block_break();
            }
            Event::Start(Tag::Item) => {
                if !self.html.is_empty() && !self.html.ends_with('\n') {
                    self.html.push('\n');
                }
                let depth = self.list_stack.len().saturating_sub(1);
                self.html.push_str(&"  ".repeat(depth));
                if let Some(state) = self.list_stack.last_mut() {
                    if state.ordered {
                        self.html.push_str(&format!("{}. ", state.next_index));
                        state.next_index += 1;
                    } else {
                        self.html.push_str("- ");
                    }
                } else {
                    self.html.push_str("- ");
                }
            }
            Event::End(Tag::Item) => self.push_line_break(),
            Event::Start(Tag::Link(_, dest_url, _)) => {
                self.html
                    .push_str(&format!("<a href=\"{}\">", escape_html(&dest_url)));
            }
            Event::End(Tag::Link(..)) => {
                self.html.push_str("</a>");
            }
            Event::Start(Tag::Image(_, dest_url, _)) => {
                self.html
                    .push_str(&escape_html(&format!("Image: {}", dest_url)));
            }
            Event::End(Tag::Image(..)) => {}
            Event::Text(text) | Event::Html(text) => self.html.push_str(&escape_html(&text)),
            Event::Code(text) => {
                self.html
                    .push_str(&format!("<code>{}</code>", escape_html(&text)));
            }
            Event::SoftBreak | Event::HardBreak => self.push_line_break(),
            Event::Rule => {
                self.push_block_break();
                self.html.push_str("----");
                self.push_block_break();
            }
            Event::FootnoteReference(label) => {
                self.html.push_str(&escape_html(&format!("[{label}]")));
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    self.html.push_str("[x] ");
                } else {
                    self.html.push_str("[ ] ");
                }
            }
            Event::Start(_) | Event::End(_) => {}
        }
    }

    fn handle_unsupported_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(_) => {
                self.unsupported_depth += 1;
            }
            Event::End(_) => {
                self.unsupported_depth = self.unsupported_depth.saturating_sub(1);
                if self.unsupported_depth == 0 {
                    self.flush_unsupported_block();
                }
            }
            Event::Text(text) | Event::Html(text) => self.unsupported_text.push_str(&text),
            Event::Code(text) => {
                self.unsupported_text.push('`');
                self.unsupported_text.push_str(&text);
                self.unsupported_text.push('`');
            }
            Event::SoftBreak | Event::HardBreak => self.unsupported_text.push('\n'),
            Event::Rule => {
                if !self.unsupported_text.is_empty() && !self.unsupported_text.ends_with('\n') {
                    self.unsupported_text.push('\n');
                }
                self.unsupported_text.push_str("----\n");
            }
            Event::FootnoteReference(label) => {
                self.unsupported_text.push('[');
                self.unsupported_text.push_str(&label);
                self.unsupported_text.push(']');
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    self.unsupported_text.push_str("[x] ");
                } else {
                    self.unsupported_text.push_str("[ ] ");
                }
            }
        }
    }

    fn flush_unsupported_block(&mut self) {
        let text = self.unsupported_text.trim().to_owned();
        if text.is_empty() {
            self.unsupported_text.clear();
            return;
        }
        self.push_block_break();
        self.html.push_str("<pre><code>");
        self.html.push_str(&escape_html(&text));
        self.html.push_str("</code></pre>");
        self.push_block_break();
        self.unsupported_text.clear();
    }

    fn push_line_break(&mut self) {
        if !self.html.ends_with('\n') {
            self.html.push('\n');
        }
    }

    fn push_block_break(&mut self) {
        if self.html.is_empty() {
            return;
        }
        if self.html.ends_with("\n\n") {
            return;
        }
        if self.html.ends_with('\n') {
            self.html.push('\n');
        } else {
            self.html.push_str("\n\n");
        }
    }

    fn finish(mut self) -> String {
        if self.unsupported_depth > 0 {
            self.unsupported_depth = 0;
            self.flush_unsupported_block();
        }
        self.html.trim().to_owned()
    }
}

fn is_unsupported_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
    )
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        OVERFLOW_NOTICE, TelegramReplyPlan, first_preview_snippet, plan_final_assistant_reply,
    };

    #[test]
    fn renders_supported_markdown_to_html() {
        let plan = plan_final_assistant_reply(
            "# Heading\n\nUse `cargo test` in **this** repo.\n\n- one\n- two",
            4096,
        );
        let TelegramReplyPlan::InlineHtml { text } = plan else {
            panic!("expected inline html");
        };

        assert!(text.contains("<b>Heading</b>"));
        assert!(text.contains("<code>cargo test</code>"));
        assert!(text.contains("<b>this</b>"));
        assert!(text.contains("- one"));
        assert!(text.contains("- two"));
    }

    #[test]
    fn escapes_raw_html() {
        let plan = plan_final_assistant_reply("<script>alert(1)</script>", 4096);
        let TelegramReplyPlan::InlineHtml { text } = plan else {
            panic!("expected inline html");
        };

        assert!(text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!text.contains("<script>"));
    }

    #[test]
    fn unsupported_blocks_fallback_to_code_block() {
        let plan = plan_final_assistant_reply("> nested\n> quote", 4096);
        let TelegramReplyPlan::InlineHtml { text } = plan else {
            panic!("expected inline html");
        };

        assert!(text.contains("<pre><code>nested\nquote</code></pre>"));
    }

    #[test]
    fn oversized_replies_switch_to_markdown_attachment() {
        let raw = "a".repeat(5000);
        let plan = plan_final_assistant_reply(&raw, 4096);
        let TelegramReplyPlan::MarkdownAttachment {
            notice_text,
            markdown,
        } = plan
        else {
            panic!("expected markdown attachment");
        };

        assert!(notice_text.starts_with(OVERFLOW_NOTICE));
        assert_eq!(markdown, raw);
    }

    #[test]
    fn preview_snippet_uses_first_non_empty_paragraph() {
        let snippet = first_preview_snippet("\n\nFirst paragraph here.\n\nSecond paragraph");
        assert_eq!(snippet, "First paragraph here.");
    }

    #[test]
    fn empty_reply_stays_plain_text() {
        let plan = plan_final_assistant_reply("   ", 4096);
        assert_eq!(
            plan,
            TelegramReplyPlan::InlinePlainText {
                text: String::new(),
                reason: "empty_reply",
            }
        );
    }
}
