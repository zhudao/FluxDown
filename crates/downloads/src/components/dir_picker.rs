//! 目录选择：`new_download.rs` 与 `quick_capture.rs` 共用的系统目录选择框封装。
//!
//! 调用方自行持有一个 `picking` 标志避免重入；本函数只负责弹框、等待结果并把
//! 选中路径（或 `None`）连同调用方状态一起送回 `on_result`。

use gpui::{Context, PathPromptOptions, SharedString, Window};

/// 弹出系统目录选择框；完成后在调用方实体上执行 `on_result(picked, view, window, cx)`。
/// 用户取消或选择失败时 `picked` 为 `None`。
pub(crate) fn pick_directory<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    prompt: SharedString,
    on_result: impl FnOnce(Option<String>, &mut V, &mut Window, &mut Context<V>) + 'static,
) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(prompt),
    });
    cx.spawn_in(window, async move |this, cx| {
        let picked = match receiver.await {
            Ok(Ok(Some(paths))) => paths.first().map(|path| path.display().to_string()),
            _ => None,
        };
        let _ = this.update_in(cx, |view, window, cx| on_result(picked, view, window, cx));
    })
    .detach();
}
