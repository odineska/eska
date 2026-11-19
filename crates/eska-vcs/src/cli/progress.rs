//! Render Gitoxide's counters through its line renderer, with terminal policy owned by the CLI.

use std::{
    io::{self, IsTerminal as _},
    sync::{Arc, Weak},
    time::Duration,
};

use prodash::{
    Root, WeakRoot,
    messages::{Message, MessageCopyState, MessageLevel},
    progress::{Id, Key, Task},
    render::line::{self, Options, StreamKind},
    tree,
};

use super::localization::Localizer;
use crate::vcs::network::Progress;

// Gitoxide assigns stable IDs to the counters; completed measurements carry the native task name.
const STAGES: &[(Id, &str, &str)] = &[
    (*b"FERP", "remote", "git-progress-remote"),
    (*b"BWRB", "read pack", "git-progress-pack"),
    (*b"BWCI", "create index file", "git-progress-index"),
    (*b"IWIO", "indexing", "git-progress-objects"),
    (*b"IWDB", "decompressing", "git-progress-inflate"),
    (*b"IWDB", "Decoding", "git-progress-inflate"),
    (*b"IWRO", "Resolving", "git-progress-deltas"),
    (*b"IWBW", "writing index file", "git-progress-write-index"),
    (*b"CLCF", "checkout", "git-progress-checkout"),
    (*b"CLCB", "writing", "git-progress-write-files"),
];

pub(super) fn run<T>(localizer: &Localizer, label: &str, run: impl FnOnce(Progress) -> T) -> T {
    let stderr = io::stderr();
    if !stderr.is_terminal() || std::env::var_os("TERM").is_some_and(|term| term == "dumb") {
        return run(Progress::from(None));
    }

    let view = ProgressView::new(localizer);
    let progress = view.tree.add_child(localizer.text(label));
    let options = Options {
        level_filter: Some(1..=3),
        initial_delay: Some(Duration::from_secs(1)),
        timestamp: true,
        throughput: true,
        ..Options::default()
    }
    .auto_configure(StreamKind::Stderr);
    let renderer = line::render(stderr, view.downgrade(), options);
    let result = run(Progress::from(Some(progress)));
    renderer.shutdown_and_wait();
    result
}

type Stages = Arc<[(Id, &'static str, String)]>;

struct ProgressView {
    tree: Arc<tree::Root>,
    stages: Stages,
}

struct WeakProgressView {
    tree: Weak<tree::Root>,
    stages: Stages,
}

impl WeakRoot for WeakProgressView {
    type Root = ProgressView;

    fn upgrade(&self) -> Option<ProgressView> {
        Some(ProgressView {
            tree: self.tree.upgrade()?,
            stages: self.stages.clone(),
        })
    }
}

impl Root for ProgressView {
    type WeakRoot = WeakProgressView;

    fn messages_capacity(&self) -> usize {
        self.tree.messages_capacity()
    }

    fn num_tasks(&self) -> usize {
        self.tree.num_tasks()
    }

    fn sorted_snapshot(&self, out: &mut Vec<(Key, Task)>) {
        self.tree.sorted_snapshot(out);
        out.retain_mut(|(_, task)| {
            let Some((_, _, label)) = self.stages.iter().find(|(id, _, _)| *id == task.id) else {
                // Leave the terminal to credential prompts until transfer starts.
                return false;
            };
            task.name.clone_from(label);
            true
        });
    }

    fn copy_messages(&self, out: &mut Vec<Message>) {
        self.tree.copy_messages(out);
        self.localize_measurements(out);
    }

    fn copy_new_messages(
        &self,
        out: &mut Vec<Message>,
        prev: Option<MessageCopyState>,
    ) -> MessageCopyState {
        let state = self.tree.copy_new_messages(out, prev);
        self.localize_measurements(out);
        state
    }

    fn downgrade(&self) -> WeakProgressView {
        WeakProgressView {
            tree: Arc::downgrade(&self.tree),
            stages: self.stages.clone(),
        }
    }
}

impl ProgressView {
    fn new(localizer: &Localizer) -> Self {
        Self {
            tree: tree::Root::new(),
            stages: STAGES
                .iter()
                .map(|&(id, native, key)| (id, native, localizer.text(key)))
                .collect(),
        }
    }

    fn localize_measurements(&self, messages: &mut Vec<Message>) {
        messages.retain_mut(|message| {
            // Diagnostics use ESKA's structured errors. Remote sideband text may contain
            // terminal escapes or credentials, so display only native measurements here.
            if message.level == MessageLevel::Failure {
                return false;
            }
            let Some((_, _, label)) = self
                .stages
                .iter()
                .find(|(_, native, _)| *native == message.origin)
            else {
                return false;
            };
            message.origin.clone_from(label);
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::localization::Locale;

    #[test]
    fn line_renderer_sees_localized_counters_but_not_remote_messages() {
        for (locale, checkout, remote) in [
            (Locale::EnUs, "Checking out", "Remote"),
            (Locale::RuRu, "Извлечение файлов", "Сервер"),
        ] {
            let view = ProgressView::new(&Localizer::try_new(locale).expect("built-in locale"));
            let _handshake = view.tree.add_child("authentication");
            let files = view.tree.add_child_with_id("checkout", *b"CLCF");
            let server = view
                .tree
                .add_child_with_id("remote: \x1b[2J arbitrary text", *b"FERP");
            server.message(MessageLevel::Failure, "untrusted server diagnostic");
            files.message(MessageLevel::Info, "done 17 files in 0.02s (850 files/s)");

            let mut tasks = Vec::new();
            view.sorted_snapshot(&mut tasks);
            let labels: Vec<_> = tasks.iter().map(|(_, task)| task.name.as_str()).collect();
            assert_eq!(labels, [checkout, remote]);

            let mut messages = Vec::new();
            let state = view.copy_new_messages(&mut messages, None);
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].origin, checkout);
            assert!(messages[0].message.contains("850 files/s"));
            view.copy_new_messages(&mut messages, Some(state));
            assert_eq!(messages, []);
        }
    }
}
