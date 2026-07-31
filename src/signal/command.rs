use crate::data::Thread;

/// How many renderable messages a page aims to produce.
pub const PAGE: u32 = 50;

#[derive(Debug, Clone)]
pub enum Command {
    SendText {
        thread: Thread,
        body: String,
        timestamp: u64,
    },
    LoadThread {
        thread: Thread,
        /// Loads the messages immediately older than this timestamp; `None`
        /// loads the newest page.
        before: Option<u64>,
    },
}

impl Command {
    pub fn load(thread: Thread) -> Self {
        Self::LoadThread {
            thread,
            before: None,
        }
    }
}
