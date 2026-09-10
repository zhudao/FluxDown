//! 剪贴板监听：`general.clipboard_watch` 打开时，检测复制的下载链接并提交给捕获队列
//! （非静默 —— 会弹出快速捕获窗口供用户确认，而非直接静默建任务）。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fluxdown_protocol::method;
use gpui::{App, Global};

use crate::agent_client::AgentClient;
use crate::app::Desktop;
use crate::launch;

const POLL_INTERVAL: Duration = Duration::from_millis(1000);

struct ClipboardWatchState {
    /// 上次读到的剪贴板文本，用于跳过未变化的内容（含启动时的基线，不触发提交）。
    last_text: Option<String>,
    seen: SeenUrls,
}

impl Global for ClipboardWatchState {}

/// 安装剪贴板监听。启动时先读一次剪贴板作为基线（不触发提交），随后每秒轮询一次；
/// 是否实际检测取决于当次轮询时的 `general.clipboard_watch` 偏好值。
pub fn install(cx: &mut App) {
    if cx.has_global::<ClipboardWatchState>() {
        return;
    }
    let baseline = cx.read_from_clipboard().and_then(|item| item.text());
    cx.set_global(ClipboardWatchState {
        last_text: baseline,
        seen: SeenUrls::new(),
    });
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let mut alive = true;
            cx.update(|cx| {
                if !cx.has_global::<Desktop>() {
                    alive = false;
                    return;
                }
                tick(cx);
            });
            if !alive {
                break;
            }
        }
    })
    .detach();
}

fn tick(cx: &mut App) {
    if !Desktop::pref_bool(cx, "general.clipboard_watch", false) {
        return;
    }
    let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
        return;
    };
    {
        let Some(state) = cx.try_global::<ClipboardWatchState>() else {
            return;
        };
        if state.last_text.as_deref() == Some(text.as_str()) {
            return;
        }
    }

    let urls = extract_capture_urls(&text);
    let now = Instant::now();
    let state = cx.global_mut::<ClipboardWatchState>();
    state.last_text = Some(text);
    let mut to_submit = Vec::with_capacity(urls.len());
    for url in urls {
        let url = launch::normalize_capture_url(&url);
        if state.seen.insert(&url, now) {
            to_submit.push(url);
        }
    }
    if to_submit.is_empty() {
        return;
    }

    let client = Desktop::global(cx).client.clone();
    for url in to_submit {
        submit_capture(cx, &client, url);
    }
}

fn submit_capture(cx: &mut App, client: &Arc<AgentClient>, url: String) {
    let future = client.call::<serde_json::Value, serde_json::Value>(
        method::AGENT_CAPTURE_SUBMIT,
        Some(serde_json::json!({ "request": { "url": url }, "silent": false })),
    );
    cx.spawn(async move |_cx| {
        let _ = future.await;
    })
    .detach();
}

/// 从剪贴板文本中提取可捕获的下载链接：整段是单个可捕获链接，或按行拆分后每一行都是
/// 可捕获链接（逐行返回）；否则返回空。
pub(crate) fn extract_capture_urls(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() > 1 {
        if lines.iter().all(|line| launch::is_capture_url(line)) {
            return lines.into_iter().map(str::to_owned).collect();
        }
        return Vec::new();
    }
    if launch::is_capture_url(trimmed) {
        return vec![trimmed.to_owned()];
    }
    Vec::new()
}

/// 最近提交过的 URL 集合：30 分钟内去重，最多保留 50 条（超出淘汰最旧的）。
pub(crate) struct SeenUrls {
    entries: VecDeque<(String, Instant)>,
}

impl SeenUrls {
    const CAPACITY: usize = 50;
    const TTL: Duration = Duration::from_secs(30 * 60);

    pub(crate) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// `url` 在 TTL 内已出现过则返回 `false`（去重命中）；否则记录并返回 `true`。
    pub(crate) fn insert(&mut self, url: &str, now: Instant) -> bool {
        self.entries
            .retain(|(_, seen_at)| now.saturating_duration_since(*seen_at) < Self::TTL);
        if self.entries.iter().any(|(seen, _)| seen == url) {
            return false;
        }
        while self.entries.len() >= Self::CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back((url.to_owned(), now));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_url() {
        let urls = extract_capture_urls("  https://example.com/file.zip  ");
        assert_eq!(urls, vec!["https://example.com/file.zip".to_owned()]);
    }

    #[test]
    fn extract_magnet_url() {
        let urls = extract_capture_urls("magnet:?xt=urn:btih:abcdef");
        assert_eq!(urls, vec!["magnet:?xt=urn:btih:abcdef".to_owned()]);
    }

    #[test]
    fn extract_multiline_all_urls() {
        let text = "https://a.example.com/1.zip\nhttps://b.example.com/2.zip\n";
        let urls = extract_capture_urls(text);
        assert_eq!(
            urls,
            vec![
                "https://a.example.com/1.zip".to_owned(),
                "https://b.example.com/2.zip".to_owned(),
            ]
        );
    }

    #[test]
    fn extract_multiline_mixed_rejected() {
        let text = "https://a.example.com/1.zip\nnot a url\n";
        assert!(extract_capture_urls(text).is_empty());
    }

    #[test]
    fn extract_plain_text_rejected() {
        assert!(extract_capture_urls("just some copied text").is_empty());
    }

    #[test]
    fn extract_empty_rejected() {
        assert!(extract_capture_urls("   \n  ").is_empty());
    }

    #[test]
    fn seen_urls_dedupes_within_ttl() {
        let mut seen = SeenUrls::new();
        let now = Instant::now();
        assert!(seen.insert("https://example.com/a", now));
        assert!(!seen.insert("https://example.com/a", now + Duration::from_secs(60)));
    }

    #[test]
    fn seen_urls_expires_after_ttl() {
        let mut seen = SeenUrls::new();
        let now = Instant::now();
        assert!(seen.insert("https://example.com/a", now));
        let later = now + SeenUrls::TTL + Duration::from_secs(1);
        assert!(seen.insert("https://example.com/a", later));
    }

    #[test]
    fn seen_urls_evicts_oldest_beyond_capacity() {
        let mut seen = SeenUrls::new();
        let now = Instant::now();
        for i in 0..SeenUrls::CAPACITY {
            assert!(seen.insert(&format!("https://example.com/{i}"), now));
        }
        // 容量已满：再插入一条新的，应淘汰最旧的一条（index 0）。
        assert!(seen.insert("https://example.com/new", now));
        assert!(seen.insert("https://example.com/0", now));
    }
}
