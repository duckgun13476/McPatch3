use std::collections::LinkedList;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::ser::SerializeMap;
use serde::Serialize;
use tokio::sync::broadcast;

pub const MAX_LOGS: usize = 1000;

#[derive(PartialEq)]
enum Mode {
    Cli,
    Webui,
}

/// 代表一个日志缓冲区。负责收集各种任务运行中的输出
#[derive(Clone)]
pub struct Console {
    inner: Arc<Mutex<Inner>>,
    sender: broadcast::Sender<LogOutputed>,
}

impl Console {
    pub fn new_cli() -> Self {
        let (sender, _) = broadcast::channel(MAX_LOGS * 2);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buf: LinkedList::new(),
                mode: Mode::Cli,
            })),
            sender,
        }
    }

    pub fn new_webui() -> Self {
        let (sender, _) = broadcast::channel(MAX_LOGS * 2);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buf: LinkedList::new(),
                mode: Mode::Webui,
            })),
            sender,
        }
    }

    /// Atomically captures the current log buffer and subscribes to future entries.
    /// Holding the buffer lock while subscribing prevents a gap between snapshot and stream.
    pub fn snapshot_and_subscribe(&self) -> (Vec<LogOutputed>, broadcast::Receiver<LogOutputed>) {
        let lock = self.inner.lock().unwrap();
        let receiver = self.sender.subscribe();
        let snapshot = lock
            .buf
            .iter()
            .map(|line| LogOutputed {
                time: line.time,
                content: line.content.clone(),
                level: line.level,
            })
            .collect();
        (snapshot, receiver)
    }

    /// 获取目前的日志。
    ///
    /// + 若`full`为true，则获取所有的日志
    /// + 若`full`为false，则获取从上次调用此方法以来的新产生的日志
    pub fn get_logs<'a>(&'a self, full: bool) -> Vec<LogOutputed> {
        let mut lock = self.inner.lock().unwrap();

        let mut entries = Vec::<LogOutputed>::new();

        if full {
            for log in &mut lock.buf {
                log.read = true;
            }

            for line in &lock.buf {
                entries.push(LogOutputed {
                    time: line.time,
                    content: line.content.to_owned(),
                    level: line.level.clone(),
                });
            }
        } else {
            for line in &lock.buf {
                if !line.read {
                    entries.push(LogOutputed {
                        time: line.time,
                        content: line.content.to_owned(),
                        level: line.level.clone(),
                    });
                }
            }

            for log in &mut lock.buf {
                log.read = true;
            }
        }

        entries
    }

    /// 记录一条“调试”日志
    pub fn log_debug(&self, content: impl AsRef<str>) {
        self.log(content, LogLevel::Debug);
    }

    /// 记录一条“普通”日志
    pub fn log_info(&self, content: impl AsRef<str>) {
        self.log(content, LogLevel::Info);
    }

    /// 记录一条“警告”日志
    pub fn log_warning(&self, content: impl AsRef<str>) {
        self.log(content, LogLevel::Warning);
    }

    /// 记录一条“错误”日志
    pub fn log_error(&self, content: impl AsRef<str>) {
        self.log(content, LogLevel::Error);
    }

    /// 记录一条日志
    fn log(&self, content: impl AsRef<str>, level: LogLevel) {
        let mut lock = self.inner.lock().unwrap();

        for line in content.as_ref().split("\n") {
            println!("{}", line);

            if lock.mode == Mode::Webui {
                let entry = Line::new(line.to_owned(), level);
                let output = LogOutputed {
                    time: entry.time,
                    content: entry.content.clone(),
                    level: entry.level,
                };
                lock.buf.push_back(entry);

                while lock.buf.len() > MAX_LOGS {
                    lock.buf.pop_front();
                }

                let _ = self.sender.send(output);
            }
        }
    }
}

pub struct Inner {
    pub buf: LinkedList<Line>,
    mode: Mode,
}

/// 代表单条日志，序列化专用
#[derive(Clone)]
pub struct LogOutputed {
    /// 日志的产生时间
    pub time: SystemTime,

    /// 日志的内容
    pub content: String,

    /// 日志的重要等级
    pub level: LogLevel,
}

impl Serialize for LogOutputed {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let unix_ts = self
            .time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("time", &unix_ts)?;
        map.serialize_entry("content", &self.content)?;
        map.serialize_entry("level", &self.level)?;
        map.end()
    }
}

#[derive(Clone)]
pub struct Line {
    /// 这条日志被阅读过吗
    pub read: bool,

    /// 日志的产生时间
    pub time: SystemTime,

    /// 日志的内容
    pub content: String,

    /// 日志的重要等级
    pub level: LogLevel,
}

impl Line {
    pub fn new(content: String, level: LogLevel) -> Self {
        Self {
            read: false,
            time: SystemTime::now(),
            content,
            level,
        }
    }
}

#[derive(Clone, Copy)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl Serialize for LogLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let text = match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        };

        serializer.collect_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Console;

    #[tokio::test]
    async fn stream_snapshot_has_no_gap_before_live_entries() {
        let console = Console::new_webui();
        console.log_info("before subscribe");

        let (snapshot, mut receiver) = console.snapshot_and_subscribe();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].content, "before subscribe");

        console.log_warning("after subscribe");
        let live = receiver.recv().await.unwrap();
        assert_eq!(live.content, "after subscribe");
    }
}
