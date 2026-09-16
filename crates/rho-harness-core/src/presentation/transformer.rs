use std::sync::Arc;

pub trait DisplayTransformer: Send + Sync {
    fn transform(&self, text: &str) -> String;
}

#[derive(Default, Clone)]
pub struct DisplayTransformerPipeline {
    transformers: Vec<Arc<dyn DisplayTransformer>>,
}

impl DisplayTransformerPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_transformer(mut self, transformer: Arc<dyn DisplayTransformer>) -> Self {
        self.transformers.push(transformer);
        self
    }

    pub fn add(&mut self, transformer: Arc<dyn DisplayTransformer>) {
        self.transformers.push(transformer);
    }

    pub fn is_empty(&self) -> bool {
        self.transformers.is_empty()
    }

    pub fn transform(&self, text: &str) -> String {
        let mut current = text.to_string();
        for t in &self.transformers {
            current = t.transform(&current);
        }
        current
    }
}

pub struct ReplaceTransformer {
    from: String,
    to: String,
}

impl ReplaceTransformer {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl DisplayTransformer for ReplaceTransformer {
    fn transform(&self, text: &str) -> String {
        text.replace(&self.from, &self.to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_transformer_pipeline_applies_in_order() {
        let mut pipeline = DisplayTransformerPipeline::new();
        pipeline.add(Arc::new(ReplaceTransformer::new("foo", "bar")));
        pipeline.add(Arc::new(ReplaceTransformer::new("bar", "baz")));

        let input = "I will call `foo` to find results.";
        let output = pipeline.transform(input);
        assert_eq!(output, "I will call `baz` to find results.");
    }

    #[test]
    fn empty_pipeline_returns_unchanged_text() {
        let pipeline = DisplayTransformerPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.transform("hello world"), "hello world");
    }
}
