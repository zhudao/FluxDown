//! 应用级动作：窗口 / 设置 / 退出 / 更新 / 日志 / 关于。

use gpui::actions;

actions!(
    app,
    [
        OpenSettings,
        Quit,
        ShowMainWindow,
        CheckUpdate,
        OpenLogsFolder,
        OpenWebsite,
        About
    ]
);
