//! Per-server console hub: ring buffer + capture file + batched, resumable streaming.
//!
//! Producers (process readers) never block: lines are appended under a short lock and
//! broadcast; a slow subscriber lags and is re-synchronised from the ring buffer, with an
//! explicit `gap` when lines have already been evicted.

use super::dialect::{LogLevel, is_continuation, parse_line};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleStream {
    Stdout,
    Stderr,
    /// Messages from MCPanel itself ("Starting server…").
    System,
    /// A command the user sent.
    Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub seq: u64,
    pub at: Timestamp,
    pub stream: ConsoleStream,
    pub level: Option<LogLevel>,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsoleBatch {
    pub lines: Vec<ConsoleLine>,
    /// Inclusive range of sequence numbers that were evicted before delivery.
    pub gap: Option<(u64, u64)>,
}

/// Maximum characters stored per line (protects memory against pathological output).
pub const MAX_LINE_CHARS: usize = 16 * 1024;

struct Inner {
    ring: VecDeque<ConsoleLine>,
    capacity: usize,
    next_seq: u64,
    last_level: Option<LogLevel>,
    capture: Option<mpsc::Sender<String>>,
    capture_dropped: u64,
}

pub struct ConsoleHub {
    inner: Mutex<Inner>,
    tx: broadcast::Sender<ConsoleLine>,
}

impl ConsoleHub {
    pub fn new(capacity: usize) -> Arc<Self> {
        let (tx, _) = broadcast::channel(4096);
        Arc::new(Self {
            inner: Mutex::new(Inner {
                ring: VecDeque::with_capacity(capacity.min(4096)),
                capacity: capacity.max(100),
                next_seq: 1,
                last_level: None,
                capture: None,
                capture_dropped: 0,
            }),
            tx,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start writing lines to a new capture file in `dir` (one file per process session).
    /// Keeps the newest `keep` capture files.
    pub fn start_capture(&self, dir: PathBuf, keep: usize) {
        let (tx, mut rx) = mpsc::channel::<String>(8192);
        self.lock().capture = Some(tx);
        tokio::spawn(async move {
            if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                tracing::warn!(target: "mcpanel::server", "cannot create console dir: {e}");
                return;
            }
            prune_captures(&dir, keep.saturating_sub(1)).await;
            let path = dir.join(format!("console-{}.log", Timestamp::now().millis()));
            let file = match tokio::fs::File::create(&path).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(target: "mcpanel::server", "cannot create console capture: {e}");
                    return;
                }
            };
            let mut w = tokio::io::BufWriter::new(file);
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    line = rx.recv() => match line {
                        Some(l) => {
                            if w.write_all(l.as_bytes()).await.is_err() || w.write_all(b"\n").await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    },
                    _ = tick.tick() => { let _ = w.flush().await; }
                }
            }
            let _ = w.flush().await;
        });
    }

    pub fn stop_capture(&self) {
        self.lock().capture = None;
    }

    /// Append a line. Never blocks on subscribers or disk.
    pub fn push(&self, stream: ConsoleStream, raw: &str) -> ConsoleLine {
        let mut text: String = raw.trim_end_matches(['\r', '\n']).to_string();
        if text.len() > MAX_LINE_CHARS {
            let mut cut = MAX_LINE_CHARS;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str(" …[truncated]");
        }
        let mut inner = self.lock();
        let level = match stream {
            ConsoleStream::Stdout | ConsoleStream::Stderr => {
                let parsed = parse_line(&text).level;
                match parsed {
                    Some(l) => Some(l),
                    None if is_continuation(&text) => inner.last_level,
                    None if stream == ConsoleStream::Stderr => Some(LogLevel::Error),
                    None => None,
                }
            }
            _ => None,
        };
        if matches!(stream, ConsoleStream::Stdout | ConsoleStream::Stderr) {
            inner.last_level = level;
        }
        let line = ConsoleLine {
            seq: inner.next_seq,
            at: Timestamp::now(),
            stream,
            level,
            text,
        };
        inner.next_seq += 1;
        if inner.ring.len() >= inner.capacity {
            inner.ring.pop_front();
        }
        inner.ring.push_back(line.clone());
        if let Some(cap) = &inner.capture {
            let formatted = match stream {
                ConsoleStream::Stdout => line.text.clone(),
                ConsoleStream::Stderr => format!("[stderr] {}", line.text),
                ConsoleStream::System => format!("[mcpanel] {}", line.text),
                ConsoleStream::Command => format!("> {}", line.text),
            };
            if cap.try_send(formatted).is_err() {
                inner.capture_dropped += 1;
            }
        }
        // Sent under the lock so subscribe() snapshots are consistent with the stream.
        let _ = self.tx.send(line.clone());
        line
    }

    pub fn last_seq(&self) -> u64 {
        self.lock().next_seq - 1
    }

    /// Lines with `seq >= from` (bounded by `limit`, newest kept).
    pub fn snapshot(&self, from: Option<u64>, limit: usize) -> Vec<ConsoleLine> {
        let inner = self.lock();
        let from = from.unwrap_or(0);
        let lines: Vec<_> = inner
            .ring
            .iter()
            .filter(|l| l.seq >= from)
            .cloned()
            .collect();
        let skip = lines.len().saturating_sub(limit);
        lines.into_iter().skip(skip).collect()
    }

    pub fn clear(&self) {
        self.lock().ring.clear();
    }

    /// Case-insensitive substring search over the buffer (newest last).
    pub fn search(&self, query: &str, limit: usize) -> Vec<ConsoleLine> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let inner = self.lock();
        let mut hits: Vec<_> = inner
            .ring
            .iter()
            .rev()
            .filter(|l| l.text.to_lowercase().contains(&q))
            .take(limit)
            .cloned()
            .collect();
        hits.reverse();
        hits
    }

    /// Subscribe starting after `after_seq` (None → only new lines plus the last
    /// `backlog` lines).
    pub fn subscribe(
        self: &Arc<Self>,
        after_seq: Option<u64>,
        backlog: usize,
    ) -> ConsoleSubscription {
        let inner = self.lock();
        let rx = self.tx.subscribe();
        let pending: VecDeque<ConsoleLine> = match after_seq {
            Some(s) => inner.ring.iter().filter(|l| l.seq > s).cloned().collect(),
            None => {
                let skip = inner.ring.len().saturating_sub(backlog);
                inner.ring.iter().skip(skip).cloned().collect()
            }
        };
        let oldest = inner.ring.front().map(|l| l.seq);
        let gap = match (after_seq, oldest) {
            (Some(s), Some(o)) if o > s + 1 => Some((s + 1, o - 1)),
            _ => None,
        };
        let last_seq = pending
            .back()
            .map(|l| l.seq)
            .or(after_seq)
            .unwrap_or(inner.next_seq - 1);
        drop(inner);
        ConsoleSubscription {
            hub: Arc::clone(self),
            rx,
            pending,
            last_seq,
            initial_gap: gap,
        }
    }
}

async fn prune_captures(dir: &std::path::Path, keep: usize) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut files = Vec::new();
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("console-") && name.ends_with(".log") {
            files.push(e.path());
        }
    }
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for f in files.into_iter().take(excess) {
        let _ = tokio::fs::remove_file(f).await;
    }
}

pub struct ConsoleSubscription {
    hub: Arc<ConsoleHub>,
    rx: broadcast::Receiver<ConsoleLine>,
    pending: VecDeque<ConsoleLine>,
    last_seq: u64,
    initial_gap: Option<(u64, u64)>,
}

impl ConsoleSubscription {
    /// Wait for at least one line, then collect for up to `max_wait` or `max_lines`.
    /// Returns `None` when the hub is gone.
    pub async fn next_batch(
        &mut self,
        max_lines: usize,
        max_wait: Duration,
    ) -> Option<ConsoleBatch> {
        let mut batch = ConsoleBatch {
            gap: self.initial_gap.take(),
            ..Default::default()
        };
        while let Some(l) = self.pending.pop_front() {
            self.last_seq = self.last_seq.max(l.seq);
            batch.lines.push(l);
            if batch.lines.len() >= max_lines {
                return Some(batch);
            }
        }
        if !batch.lines.is_empty() || batch.gap.is_some() {
            return Some(batch);
        }
        // Block for the first line.
        match self.recv_one(&mut batch).await {
            Ok(()) => {}
            Err(()) => return None,
        }
        let deadline = tokio::time::Instant::now() + max_wait;
        while batch.lines.len() < max_lines {
            match tokio::time::timeout_at(deadline, self.recv_one(&mut batch)).await {
                Ok(Ok(())) => {}
                Ok(Err(())) => break,
                Err(_) => break,
            }
        }
        Some(batch)
    }

    async fn recv_one(&mut self, batch: &mut ConsoleBatch) -> Result<(), ()> {
        loop {
            match self.rx.recv().await {
                Ok(line) => {
                    if line.seq <= self.last_seq {
                        continue;
                    }
                    self.last_seq = line.seq;
                    batch.lines.push(line);
                    return Ok(());
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Re-sync from the ring buffer.
                    let resync = self.hub.snapshot(Some(self.last_seq + 1), usize::MAX);
                    if let Some(first) = resync.first()
                        && first.seq > self.last_seq + 1
                    {
                        batch.gap = Some((self.last_seq + 1, first.seq - 1));
                    }
                    if let Some(last) = resync.last() {
                        self.last_seq = last.seq;
                    }
                    let got = !resync.is_empty();
                    batch.lines.extend(resync);
                    if got {
                        return Ok(());
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return Err(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_and_assigns_sequences() {
        let hub = ConsoleHub::new(100);
        for i in 0..150 {
            hub.push(ConsoleStream::Stdout, &format!("line {i}"));
        }
        let snap = hub.snapshot(None, usize::MAX);
        assert_eq!(snap.len(), 100);
        assert_eq!(snap[0].seq, 51);
        assert_eq!(hub.last_seq(), 150);
    }

    #[test]
    fn continuation_inherits_level() {
        let hub = ConsoleHub::new(100);
        hub.push(
            ConsoleStream::Stdout,
            "[12:00:00] [Server thread/ERROR]: Boom",
        );
        let l = hub.push(
            ConsoleStream::Stdout,
            "\tat net.minecraft.Foo.bar(Foo.java:1)",
        );
        assert_eq!(l.level, Some(LogLevel::Error));
    }

    #[test]
    fn truncates_huge_lines() {
        let hub = ConsoleHub::new(100);
        let l = hub.push(ConsoleStream::Stdout, &"x".repeat(100_000));
        assert!(l.text.len() < MAX_LINE_CHARS + 20);
    }

    #[tokio::test]
    async fn subscription_batches_and_resumes() {
        let hub = ConsoleHub::new(1000);
        hub.push(ConsoleStream::Stdout, "a");
        hub.push(ConsoleStream::Stdout, "b");
        let mut sub = hub.subscribe(None, 10);
        let b = sub
            .next_batch(100, Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(b.lines.len(), 2);
        for i in 0..5 {
            hub.push(ConsoleStream::Stdout, &format!("n{i}"));
        }
        let b = sub
            .next_batch(100, Duration::from_millis(20))
            .await
            .unwrap();
        assert_eq!(b.lines.len(), 5);
        assert_eq!(b.lines[0].text, "n0");

        let mut resumed = hub.subscribe(Some(3), 10);
        let b = resumed
            .next_batch(100, Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(b.lines.first().unwrap().seq, 4);
    }

    #[tokio::test]
    async fn lagging_subscriber_gets_gap_not_block() {
        let hub = ConsoleHub::new(200);
        let mut sub = hub.subscribe(None, 0);
        // Flood far beyond the broadcast capacity and the ring size.
        for i in 0..10_000 {
            hub.push(ConsoleStream::Stdout, &format!("flood {i}"));
        }
        let b = sub
            .next_batch(500, Duration::from_millis(10))
            .await
            .unwrap();
        assert!(b.gap.is_some(), "expected a gap marker");
        assert!(!b.lines.is_empty());
        assert_eq!(b.lines.last().unwrap().seq, 10_000);
    }
}
