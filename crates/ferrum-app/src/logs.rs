use ferrum_core::{ContainerLogLine, ContainerLogSource};
use std::collections::VecDeque;

use crate::search::CaseInsensitiveQuery;

const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_RETAINED_LINES: usize = 10_000;

#[derive(Default)]
pub(super) struct LogBuffer {
    entries: VecDeque<ContainerLogLine>,
    visible_sequences: VecDeque<u64>,
    retained_bytes: usize,
    dropped_lines: usize,
    first_sequence: u64,
    next_sequence: u64,
    query: String,
    matcher: CaseInsensitiveQuery,
    stdout: bool,
    stderr: bool,
}

impl LogBuffer {
    pub(super) fn new() -> Self {
        Self {
            stdout: true,
            stderr: true,
            ..Self::default()
        }
    }

    pub(super) fn append(&mut self, line: ContainerLogLine) {
        let bytes = line.text.len();
        if bytes > MAX_RETAINED_BYTES {
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            return;
        }
        while self.entries.len() >= MAX_RETAINED_LINES
            || self.retained_bytes.saturating_add(bytes) > MAX_RETAINED_BYTES
        {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            self.retained_bytes -= oldest.text.len();
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            self.first_sequence = self.first_sequence.saturating_add(1);
            while self
                .visible_sequences
                .front()
                .is_some_and(|sequence| *sequence < self.first_sequence)
            {
                self.visible_sequences.pop_front();
            }
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let visible = self.source_enabled(&line.source) && self.matcher.matches(&line.text);
        self.retained_bytes += bytes;
        self.entries.push_back(line);
        if visible {
            self.visible_sequences.push_back(sequence);
        }
    }

    pub(super) fn set_filters(&mut self, query: &str, stdout: bool, stderr: bool) {
        if self.query == query && self.stdout == stdout && self.stderr == stderr {
            return;
        }
        self.query.clear();
        self.query.push_str(query);
        self.stdout = stdout;
        self.stderr = stderr;
        self.matcher.set(query);
        self.rebuild_visibility();
    }

    #[cfg(test)]
    pub(super) fn visible_indices(&self) -> Vec<usize> {
        self.visible_sequences
            .iter()
            .map(|sequence| (*sequence - self.first_sequence) as usize)
            .collect()
    }
    pub(super) fn visible_count(&self) -> usize {
        self.visible_sequences.len()
    }

    pub(super) fn visible_entry_at(&self, row: usize) -> Option<&ContainerLogLine> {
        let sequence = *self.visible_sequences.get(row)?;
        let index = usize::try_from(sequence.checked_sub(self.first_sequence)?).ok()?;
        self.entries.get(index)
    }
    pub(super) fn entries(&self) -> &VecDeque<ContainerLogLine> {
        &self.entries
    }
    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(super) fn dropped_lines(&self) -> usize {
        self.dropped_lines
    }

    pub(super) fn reset(&mut self) {
        self.entries.clear();
        self.visible_sequences.clear();
        self.retained_bytes = 0;
        self.dropped_lines = 0;
        self.first_sequence = self.next_sequence;
    }

    fn source_enabled(&self, source: &ContainerLogSource) -> bool {
        match source {
            ContainerLogSource::Stdout => self.stdout,
            ContainerLogSource::Stderr => self.stderr,
            ContainerLogSource::Combined => self.stdout || self.stderr,
        }
    }

    fn rebuild_visibility(&mut self) {
        self.visible_sequences.clear();
        for (offset, line) in self.entries.iter().enumerate() {
            if self.source_enabled(&line.source) && self.matcher.matches(&line.text) {
                self.visible_sequences
                    .push_back(self.first_sequence + offset as u64);
            }
        }
    }
}
