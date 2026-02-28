use crate::data::ProfileData;
use crate::data::ThreadGroup;
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
    Ready(Box<ProfileData>),
    Error(String),
}

impl FileTab {
    pub fn stats(&self) -> Option<&ProfileData> {
        match &self.load_state {
            FileLoadState::Ready(stats) => Some(stats.as_ref()),
            _ => None,
        }
    }

    pub fn thread_groups(&self) -> &[ThreadGroup] {
        let Some(stats) = self.stats() else {
            return &[];
        };
        if stats.ui.merge_threads {
            &stats.data.merged_thread_groups
        } else {
            &stats.data.timeline.thread_groups
        }
    }

    pub fn thread_groups_mut(&mut self) -> Option<&mut [ThreadGroup]> {
        let stats = match &mut self.load_state {
            FileLoadState::Ready(stats) => stats.as_mut(),
            _ => return None,
        };
        if stats.ui.merge_threads {
            Some(&mut stats.data.merged_thread_groups)
        } else {
            Some(&mut stats.data.timeline.thread_groups)
        }
    }
}
