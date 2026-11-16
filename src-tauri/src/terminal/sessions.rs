//! The per-sandbox session table: the pure bookkeeping that keeps a detach, a superseding attach
//! and a finished session from leaking or overwriting each other
//! (`docs/features/native-terminal/02_dev.md`, The Concrete Model).

use std::collections::HashMap;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::SessionCommand;

/// One live session's handle. Dropping it closes the command queue, which ends the session task.
pub(super) struct LiveSession {
    id: u64,
    commands: mpsc::Sender<SessionCommand>,
    task: Option<JoinHandle<()>>,
}

enum Entry {
    /// An attach whose `open()` is still in flight.
    Pending { id: u64 },
    Live(LiveSession),
}

/// At most one entry per sandbox. An attach registers `Pending` before it opens anything and
/// promotes only if the entry is still its own, so a detach or a newer attach cancels it.
#[derive(Default)]
pub(super) struct SessionTable {
    next_id: u64,
    entries: HashMap<String, Entry>,
}

impl SessionTable {
    /// Registers a new attach for `name`, replacing (and so ending) any earlier entry.
    pub(super) fn begin(&mut self, name: &str) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.entries.insert(name.to_string(), Entry::Pending { id });
        id
    }

    /// Turns attach `id`'s pending entry live. False when the attach was cancelled or superseded.
    pub(super) fn promote(&mut self, name: &str, id: u64, commands: mpsc::Sender<SessionCommand>) -> bool {
        match self.entries.get(name) {
            Some(Entry::Pending { id: pending }) if *pending == id => {
                self.entries.insert(name.to_string(), Entry::Live(LiveSession { id, commands, task: None }));
                true
            }
            _ => false,
        }
    }

    /// Records the task of live session `id`, so exit cleanup can wait for it.
    pub(super) fn set_task(&mut self, name: &str, id: u64, task: JoinHandle<()>) {
        if let Some(Entry::Live(live)) = self.entries.get_mut(name) {
            if live.id == id {
                live.task = Some(task);
            }
        }
    }

    /// Removes the entry, pending or live.
    pub(super) fn detach(&mut self, name: &str) {
        self.entries.remove(name);
    }

    /// Removes the pending entry of attach `id` after its `open()` failed.
    pub(super) fn abandon(&mut self, name: &str, id: u64) {
        if matches!(self.entries.get(name), Some(Entry::Pending { id: pending }) if *pending == id) {
            self.entries.remove(name);
        }
    }

    /// Removes the live entry of session `id`, leaving a newer entry alone.
    pub(super) fn finish(&mut self, name: &str, id: u64) {
        if matches!(self.entries.get(name), Some(Entry::Live(live)) if live.id == id) {
            self.entries.remove(name);
        }
    }

    /// The command queue of the live session; a pending entry has none.
    pub(super) fn sender(&self, name: &str) -> Option<mpsc::Sender<SessionCommand>> {
        match self.entries.get(name) {
            Some(Entry::Live(live)) => Some(live.commands.clone()),
            _ => None,
        }
    }

    /// Empties the table, dropping every sender, and returns the live session tasks.
    pub(super) fn drain(&mut self) -> Vec<JoinHandle<()>> {
        self.entries
            .drain()
            .filter_map(|(_, entry)| match entry {
                Entry::Live(live) => live.task,
                Entry::Pending { .. } => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> mpsc::Sender<SessionCommand> {
        mpsc::channel(1).0
    }

    #[test]
    fn a_detach_during_pending_makes_promote_fail() {
        let mut table = SessionTable::default();
        let id = table.begin("demo");
        table.detach("demo");
        assert!(!table.promote("demo", id, queue()));
        assert!(table.sender("demo").is_none());
    }

    #[test]
    fn a_superseded_attach_cannot_overwrite_the_newer_one() {
        let mut table = SessionTable::default();
        let a = table.begin("demo");
        let b = table.begin("demo");
        assert!(table.promote("demo", b, queue()));
        assert!(!table.promote("demo", a, queue()));
        assert!(table.sender("demo").is_some());
    }

    #[test]
    fn a_stale_finish_keeps_the_newer_entry() {
        let mut table = SessionTable::default();
        let a = table.begin("demo");
        assert!(table.promote("demo", a, queue()));
        let b = table.begin("demo");
        table.finish("demo", a);
        assert!(table.promote("demo", b, queue()));
        table.finish("demo", a);
        assert!(table.sender("demo").is_some());
        table.finish("demo", b);
        assert!(table.sender("demo").is_none());
    }

    #[test]
    fn a_pending_entry_has_no_sender() {
        let mut table = SessionTable::default();
        table.begin("demo");
        assert!(table.sender("demo").is_none());
    }

    #[test]
    fn abandon_only_removes_its_own_pending_entry() {
        let mut table = SessionTable::default();
        let a = table.begin("demo");
        let b = table.begin("demo");
        table.abandon("demo", a);
        assert!(table.promote("demo", b, queue()));
    }
}
