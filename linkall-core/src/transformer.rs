//! Smart Clipboard Transformers & Processing Pipelines
//!
//! Transformers clean, normalize, or enrich clipboard content on arrival
//! or departure across devices (e.g., stripping tracking parameters from URLs,
//! normalizing line endings, or cleaning whitespace).

use crate::protocol::ClipboardContent;

/// Trait for smart clipboard content transformations.
pub trait Transformer: Send + Sync {
    fn name(&self) -> &'static str;

    /// Transform clipboard content in place.
    /// Returns `true` if the content was modified.
    fn transform(&self, content: &mut ClipboardContent) -> bool;
}

/// A pipeline of transformers run sequentially on clipboard content.
pub struct TransformerPipeline {
    transformers: Vec<Box<dyn Transformer>>,
}

impl Default for TransformerPipeline {
    fn default() -> Self {
        Self::default_pipeline()
    }
}

impl TransformerPipeline {
    pub fn new() -> Self {
        Self {
            transformers: Vec::new(),
        }
    }

    /// Creates a default pipeline containing standard privacy & normalization transformers.
    pub fn default_pipeline() -> Self {
        let mut p = Self::new();
        p.push(Box::new(UtmStripperTransformer));
        p.push(Box::new(WhitespaceCleanerTransformer));
        p
    }

    pub fn push(&mut self, transformer: Box<dyn Transformer>) {
        self.transformers.push(transformer);
    }

    /// Runs all transformers in sequence.
    /// Returns `true` if at least one transformer modified the payload.
    pub fn transform(&self, content: &mut ClipboardContent) -> bool {
        let mut modified = false;
        for t in &self.transformers {
            if t.transform(content) {
                modified = true;
            }
        }
        modified
    }
}

/// Automatically strips marketing & tracking query parameters (`utm_*`, `fbclid`, `gclid`, etc.)
/// from copied URLs.
pub struct UtmStripperTransformer;

impl UtmStripperTransformer {
    const TRACKING_PARAMS: &'static [&'static str] = &[
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "fbclid",
        "gclid",
        "dclid",
        "si",
        "igsh",
        "ref_src",
        "_hsenc",
        "_hsmi",
        "mc_cid",
        "mc_eid",
    ];

    fn clean_url(url: &str) -> String {
        let Some(q_idx) = url.find('?') else {
            return url.to_string();
        };

        let base = &url[..q_idx];
        let query_part = &url[q_idx + 1..];

        // Separate query from optional hash fragment (#...)
        let (query, fragment) = match query_part.find('#') {
            Some(h_idx) => (&query_part[..h_idx], Some(&query_part[h_idx..])),
            None => (query_part, None),
        };

        let mut retained_params = Vec::new();
        for param in query.split('&') {
            if param.is_empty() {
                continue;
            }
            let key = param.split('=').next().unwrap_or(param).to_lowercase();
            if !Self::TRACKING_PARAMS.contains(&key.as_str()) {
                retained_params.push(param);
            }
        }

        let mut result = base.to_string();
        if !retained_params.is_empty() {
            result.push('?');
            result.push_str(&retained_params.join("&"));
        }
        if let Some(frag) = fragment {
            result.push_str(frag);
        }
        result
    }

    fn clean_text(text: &str) -> (String, bool) {
        let mut modified = false;
        let mut output = String::with_capacity(text.len());
        let mut remaining = text;

        while let Some(pos) = remaining
            .find("http://")
            .or_else(|| remaining.find("https://"))
        {
            output.push_str(&remaining[..pos]);
            let url_start = &remaining[pos..];
            let end_idx = url_start
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(url_start.len());

            let url = &url_start[..end_idx];
            let cleaned = Self::clean_url(url);
            if cleaned != url {
                modified = true;
            }
            output.push_str(&cleaned);
            remaining = &url_start[end_idx..];
        }
        output.push_str(remaining);

        (output, modified)
    }
}

impl Transformer for UtmStripperTransformer {
    fn name(&self) -> &'static str {
        "UtmStripperTransformer"
    }

    fn transform(&self, content: &mut ClipboardContent) -> bool {
        match content {
            ClipboardContent::Text(text) => {
                let (cleaned, modified) = Self::clean_text(text);
                if modified {
                    *text = cleaned;
                }
                modified
            }
            _ => false,
        }
    }
}

/// Normalizes line breaks (`\r\n` -> `\n`) and trims trailing whitespace on lines.
pub struct WhitespaceCleanerTransformer;

impl Transformer for WhitespaceCleanerTransformer {
    fn name(&self) -> &'static str {
        "WhitespaceCleanerTransformer"
    }

    fn transform(&self, content: &mut ClipboardContent) -> bool {
        match content {
            ClipboardContent::Text(text) => {
                let mut modified = false;
                if text.contains("\r\n") {
                    *text = text.replace("\r\n", "\n");
                    modified = true;
                }
                let mut cleaned_lines = Vec::new();
                for line in text.split('\n') {
                    let trimmed = line.trim_end();
                    if trimmed.len() != line.len() {
                        modified = true;
                    }
                    cleaned_lines.push(trimmed);
                }
                let joined = cleaned_lines.join("\n");
                let final_text = joined.trim_end().to_string();
                if final_text != *text {
                    *text = final_text;
                    modified = true;
                }
                modified
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utm_stripper_removes_tracking_params_preserves_others() {
        let mut content = ClipboardContent::Text(
            "Check this out: https://example.com/item?id=42&utm_source=twitter&utm_medium=social&si=12345#section".into()
        );
        let transformer = UtmStripperTransformer;
        assert!(transformer.transform(&mut content));
        assert_eq!(
            content,
            ClipboardContent::Text("Check this out: https://example.com/item?id=42#section".into())
        );
    }

    #[test]
    fn utm_stripper_pure_tracking_url() {
        let mut content = ClipboardContent::Text(
            "https://shop.com/deal?utm_campaign=spring&fbclid=abc_123".into(),
        );
        let transformer = UtmStripperTransformer;
        assert!(transformer.transform(&mut content));
        assert_eq!(
            content,
            ClipboardContent::Text("https://shop.com/deal".into())
        );
    }

    #[test]
    fn whitespace_cleaner_normalizes_crlf_and_trailing_spaces() {
        let mut content = ClipboardContent::Text("let x = 10;   \r\nlet y = 20;\t\r\n".into());
        let transformer = WhitespaceCleanerTransformer;
        assert!(transformer.transform(&mut content));
        assert_eq!(
            content,
            ClipboardContent::Text("let x = 10;\nlet y = 20;".into())
        );
    }

    #[test]
    fn default_pipeline_runs_both() {
        let mut content = ClipboardContent::Text("https://test.org?utm_source=ad   \r\n".into());
        let pipeline = TransformerPipeline::default_pipeline();
        assert!(pipeline.transform(&mut content));
        assert_eq!(content, ClipboardContent::Text("https://test.org".into()));
    }
}
