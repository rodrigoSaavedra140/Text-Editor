pub struct Clipboard {
    content: String,
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard { content: String::new() }
    }

    pub fn copy(&mut self, text: &str) {
        self.content = text.to_string();
    }

    pub fn paste(&self) -> &str {
        &self.content
    }
}
