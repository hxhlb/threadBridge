use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingImageBatchEntry {
    pub added_at: String,
    pub caption: Option<String>,
    pub file_name: String,
    pub mime_type: String,
    pub relative_path: String,
    pub source_message_id: i32,
    pub telegram_file_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingImageBatch {
    pub batch_id: String,
    pub control_message_id: Option<i32>,
    pub created_at: String,
    pub images: Vec<PendingImageBatchEntry>,
    pub latest_caption: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisArtifact {
    pub batch_id: String,
    pub created_at: String,
    pub image_count: usize,
    pub images: Vec<ImageAnalysisImage>,
    pub prompt: String,
    pub result_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisImage {
    pub file_name: String,
    pub mime_type: String,
    pub relative_path: String,
    pub source_message_id: i32,
}

pub fn render_pending_image_batch(batch: &PendingImageBatch) -> String {
    let mut lines = vec![
        "Image batch queued for this thread.".to_owned(),
        format!("Images: {}", batch.images.len()),
    ];
    if let Some(caption) = batch.latest_caption.as_deref() {
        lines.push(format!("Latest hint: {caption}"));
    }
    lines.push(
        "Press the button below for direct analysis, or send your next text message as the analysis request for this image batch."
            .to_owned(),
    );
    lines.join("\n")
}

pub fn build_image_analysis_prompt(batch: &PendingImageBatch, user_prompt: Option<&str>) -> String {
    if let Some(user_prompt) = user_prompt.map(str::trim).filter(|text| !text.is_empty()) {
        let mut parts = vec![
            "Analyze the attached image batch and answer the user's request directly.".to_owned(),
            "If the user asks a question, answer that question using the images as evidence."
                .to_owned(),
            "If the user specifies a language, answer in that language.".to_owned(),
            format!("User request: {user_prompt}"),
        ];
        if let Some(caption) = batch.latest_caption.as_deref() {
            parts.push(format!("Latest image hint: {caption}"));
        }
        return parts.join("\n\n");
    }

    let mut parts = vec![
        "Please analyze this batch of images.".to_owned(),
        "Describe the main subjects, composition, visual style, lighting, mood, and prompt-building cues that matter for later image generation work.".to_owned(),
    ];
    if let Some(caption) = batch.latest_caption.as_deref() {
        parts.push(format!("User hint: {caption}"));
    }
    parts.join("\n\n")
}
