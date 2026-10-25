//! A JSON Pointer (RFC 6901) built up while walking a document.

pub(super) struct Pointer {
    text: String,
    marks: Vec<usize>,
}

impl Pointer {
    pub(super) fn new() -> Self {
        Self {
            text: String::new(),
            marks: Vec::new(),
        }
    }

    /// Appends a segment, escaping `~` and `/` as RFC 6901 requires.
    pub(super) fn push(&mut self, segment: &str) {
        self.marks.push(self.text.len());
        self.text.push('/');
        for character in segment.chars() {
            match character {
                '~' => self.text.push_str("~0"),
                '/' => self.text.push_str("~1"),
                other => self.text.push(other),
            }
        }
    }

    pub(super) fn push_index(&mut self, index: usize) {
        self.marks.push(self.text.len());
        self.text.push('/');
        self.text.push_str(&index.to_string());
    }

    pub(super) fn pop(&mut self) {
        if let Some(length) = self.marks.pop() {
            self.text.truncate(length);
        }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    /// The pointer with one more segment, without changing this one.
    pub(super) fn with(&self, segment: &str) -> String {
        let mut copy = Self {
            text: self.text.clone(),
            marks: Vec::new(),
        };
        copy.push(segment);
        copy.text
    }
}

#[cfg(test)]
mod tests {
    use super::Pointer;

    #[test]
    fn segments_are_escaped_and_popped() {
        let mut pointer = Pointer::new();
        pointer.push("doc");
        pointer.push("a/b~c");
        pointer.push_index(3);
        assert_eq!(pointer.as_str(), "/doc/a~1b~0c/3");
        pointer.pop();
        pointer.pop();
        assert_eq!(pointer.as_str(), "/doc");
        assert_eq!(pointer.with("content"), "/doc/content");
        assert_eq!(pointer.as_str(), "/doc", "with() leaves the pointer alone");
    }
}
