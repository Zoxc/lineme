use crate::data::FileTab as FileTabData;
use crate::timeline::ThreadGroup;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileTab {
    pub id: u64,
    pub path: PathBuf,
    pub load_state: FileLoadState,
}

#[derive(Debug, Clone)]
pub enum FileLoadState {
    Loading,
    Ready(FileTabData),
    Error(String),
}

impl FileTab {
    pub fn stats(&self) -> Option<&FileTabData> {
        match &self.load_state {
            FileLoadState::Ready(stats) => Some(stats),
            _ => None,
        }
    }

    pub fn thread_groups(&self) -> Option<&[ThreadGroup]> {
        let stats = self.stats()?;
        if stats.ui.merge_threads {
            Some(&stats.data.merged_thread_groups)
        } else {
            Some(&stats.data.timeline.thread_groups)
        }
    }

    pub fn thread_groups_mut(&mut self) -> Option<&mut [ThreadGroup]> {
        let stats = match &mut self.load_state {
            FileLoadState::Ready(stats) => stats,
            _ => return None,
        };
        if stats.ui.merge_threads {
            Some(&mut stats.data.merged_thread_groups)
        } else {
            Some(&mut stats.data.timeline.thread_groups)
        }
    }
}
