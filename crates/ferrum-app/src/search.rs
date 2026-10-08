#[derive(Default)]
pub(super) struct CaseInsensitiveQuery {
    needle: Vec<char>,
    prefix: Vec<usize>,
}

impl CaseInsensitiveQuery {
    pub(super) fn set(&mut self, query: &str) {
        self.needle.clear();
        self.needle
            .extend(query.chars().flat_map(char::to_lowercase));
        self.prefix.clear();
        self.prefix.resize(self.needle.len(), 0);
        let mut matched = 0;
        for index in 1..self.needle.len() {
            while matched > 0 && self.needle[index] != self.needle[matched] {
                matched = self.prefix[matched - 1];
            }
            if self.needle[index] == self.needle[matched] {
                matched += 1;
                self.prefix[index] = matched;
            }
        }
    }

    pub(super) fn matches(&self, candidate: &str) -> bool {
        if self.needle.is_empty() {
            return true;
        }
        let mut matched = 0;
        for character in candidate.chars().flat_map(char::to_lowercase) {
            while matched > 0 && character != self.needle[matched] {
                matched = self.prefix[matched - 1];
            }
            if character == self.needle[matched] {
                matched += 1;
                if matched == self.needle.len() {
                    return true;
                }
            }
        }
        false
    }
}
