use std::rc::Rc;

use fluxdown_ui_components::activity_button as activity_bar_button;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, AnyView, App, Context, Div, Entity, Img, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, img, px,
};
use gpui_component::{
    Icon, TITLE_BAR_HEIGHT, TitleBar, h_flex, menu::AppMenuBar, tooltip::Tooltip, v_flex,
};

use crate::assets::APP_LOGO_PATH;

/// shell 路由的稳定标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteId(&'static str);

impl RouteId {
    /// 创建稳定路由标识。
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }
}

/// 由应用装配并注入 shell 的路由元数据与页面内容。
pub struct ShellRoute {
    id: RouteId,
    button_id: &'static str,
    tooltip_id: &'static str,
    label_key: &'static str,
    icon: Icon,
    view: AnyView,
    /// 可选路由参与活动栏「是否整体渲染」的判定；固定路由（如下载）不参与。
    optional: bool,
    visible: bool,
}

impl ShellRoute {
    /// 创建一条 shell 路由；默认固定显示（非可选、可见）。
    pub fn new(
        id: RouteId,
        button_id: &'static str,
        tooltip_id: &'static str,
        label_key: &'static str,
        icon: Icon,
        view: AnyView,
    ) -> Self {
        Self {
            id,
            button_id,
            tooltip_id,
            label_key,
            icon,
            view,
            optional: false,
            visible: true,
        }
    }

    /// 标记该路由是否为「可选」：活动栏在没有任何可见可选项时整体收起。
    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }
}

type ShellActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;
type ShellActionIcon = Rc<dyn Fn(&App) -> Icon>;

/// 由应用装配并注入 shell 的窗口级活动栏动作。
pub struct ShellAction {
    button_id: &'static str,
    tooltip_id: &'static str,
    label_key: &'static str,
    icon: ShellActionIcon,
    handler: ShellActionHandler,
    /// 可选动作参与活动栏「是否整体渲染」的判定；固定动作（如设置）不参与。
    optional: bool,
    visible: bool,
}

impl ShellAction {
    /// 创建不切换主内容路由的活动栏动作；默认固定显示（非可选、可见）。
    pub fn new(
        button_id: &'static str,
        tooltip_id: &'static str,
        label_key: &'static str,
        icon: Icon,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::with_dynamic_icon(
            button_id,
            tooltip_id,
            label_key,
            move |_cx| icon.clone(),
            handler,
        )
    }

    /// 创建图标随运行时状态变化的活动栏动作（例如随主题模式在日/月间切换）。
    pub fn with_dynamic_icon(
        button_id: &'static str,
        tooltip_id: &'static str,
        label_key: &'static str,
        icon: impl Fn(&App) -> Icon + 'static,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            button_id,
            tooltip_id,
            label_key,
            icon: Rc::new(icon),
            handler: Rc::new(handler),
            optional: false,
            visible: true,
        }
    }

    /// 标记该动作是否为「可选」：活动栏在没有任何可见可选项时整体收起。
    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }
}

/// 使用 FluxDown 自定义标题栏承载任意能力页面的辅助窗口。
pub struct AuxiliaryWindowView {
    _translator: Entity<Translator>,
    title: SharedString,
    content: AnyView,
}

impl AuxiliaryWindowView {
    /// 创建跟随共享语言状态更新标题的辅助窗口 chrome。
    pub fn new(
        translator: Entity<Translator>,
        title_key: &'static str,
        content: AnyView,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = SharedString::from(translator.read(cx).text(title_key).to_owned());
        cx.observe(&translator, move |this, translator, cx| {
            this.title = SharedString::from(translator.read(cx).text(title_key).to_owned());
            cx.notify();
        })
        .detach();
        Self {
            _translator: translator,
            title,
            content,
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let spacing = tokens.spacing;
        let typography = tokens.typography.clone();
        let title_bar = TitleBar::new();
        #[cfg(not(target_os = "macos"))]
        let title_bar = title_bar.pl(spacing.sm);

        title_bar
            .h(TITLE_BAR_HEIGHT)
            .bg(tokens.colors.surface)
            .border_color(tokens.colors.border)
            .child(
                h_flex()
                    .size_full()
                    .min_w_0()
                    .items_center()
                    .pl(spacing.sm)
                    .pr(spacing.md)
                    .child(
                        div()
                            .min_w_0()
                            .text_size(typography.sm.size)
                            .font_weight(typography.sm.weight)
                            .child(self.title.clone()),
                    ),
            )
    }
}

impl Render for AuxiliaryWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .size_full()
            .relative()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.content.clone()),
            )
            .children(gpui_component::Root::render_sheet_layer(window, cx))
            .children(gpui_component::Root::render_dialog_layer(window, cx))
            .children(gpui_component::Root::render_notification_layer(window, cx))
    }
}

/// 活动栏轨宽度：40px 按钮居中 + 左右各 4px 留白。
const ACTIVITY_RAIL_WIDTH: gpui::Pixels = px(48.);
/// 活动栏每个按钮位所占的行高（等于按钮自身高度，纵向间距由 `gap` 提供）。
const ACTIVITY_TILE_HEIGHT: gpui::Pixels = px(40.);
/// 活动栏按钮尺寸。
const ACTIVITY_BUTTON_SIZE: gpui::Pixels = px(40.);
/// 活动栏图标尺寸。
const ACTIVITY_ICON_SIZE: gpui::Pixels = px(20.);
/// GPUI 窗口外壳：只负责窗口 chrome、活动栏、路由与内容槽位。
pub struct ShellView {
    translator: Entity<Translator>,
    active_route: Option<RouteId>,
    routes: Vec<ShellRoute>,
    actions: Vec<ShellAction>,
    /// Windows / Linux 标题栏内的应用菜单；macOS 走原生菜单不渲染。
    menu_bar: Option<Entity<AppMenuBar>>,
}

impl ShellView {
    /// 创建 shell，并使用传入顺序的首条路由作为初始页面。
    pub fn new(
        translator: Entity<Translator>,
        routes: Vec<ShellRoute>,
        actions: Vec<ShellAction>,
        menu_bar: Option<Entity<AppMenuBar>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let active_route = routes.first().map(|route| route.id);
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        Self {
            translator,
            active_route,
            routes,
            actions,
            menu_bar,
        }
    }

    /// 切换到指定路由（宿主导航入口）。
    pub fn navigate(&mut self, route: RouteId, cx: &mut Context<Self>) {
        if self.active_route != Some(route) {
            self.active_route = Some(route);
            cx.notify();
        }
    }

    /// 设置路由的可见性（宿主偏好回流入口）；隐藏当前活跃路由时自动切到首条可见路由。
    pub fn set_route_visible(&mut self, route: RouteId, visible: bool, cx: &mut Context<Self>) {
        let Some(target) = self.routes.iter_mut().find(|r| r.id == route) else {
            return;
        };
        if target.visible == visible {
            return;
        }
        target.visible = visible;
        if !visible && self.active_route == Some(route) {
            self.active_route = self.routes.iter().find(|r| r.visible).map(|r| r.id);
        }
        cx.notify();
    }

    /// 设置活动栏动作的可见性（宿主偏好回流入口）。
    pub fn set_action_visible(
        &mut self,
        button_id: &'static str,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.actions.iter_mut().find(|a| a.button_id == button_id) else {
            return;
        };
        if target.visible == visible {
            return;
        }
        target.visible = visible;
        cx.notify();
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let colors = tokens.colors;
        let spacing = tokens.spacing;
        let logo = img(APP_LOGO_PATH).size(px(16.));
        let title_bar = TitleBar::new();
        #[cfg(not(target_os = "macos"))]
        let title_bar = title_bar.pl(spacing.sm);
        #[cfg(target_os = "macos")]
        let (leading_logo, trailing_logo): (Option<Img>, Option<Img>) = (None, Some(logo));
        #[cfg(not(target_os = "macos"))]
        let (leading_logo, trailing_logo): (Option<Img>, Option<Img>) = (Some(logo), None);
        let menu_bar = if cfg!(target_os = "macos") {
            None
        } else {
            self.menu_bar.clone()
        };

        title_bar
            .h(TITLE_BAR_HEIGHT)
            .bg(colors.surface)
            .border_color(colors.border)
            .child(
                h_flex()
                    .size_full()
                    .min_w_0()
                    .items_center()
                    .gap(spacing.sm)
                    .pl(spacing.xxs)
                    .pr(spacing.md)
                    .children(leading_logo)
                    .children(menu_bar.map(|menu_bar| {
                        h_flex()
                            .h_full()
                            .items_center()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(menu_bar)
                    }))
                    .child(div().flex_1())
                    .children(trailing_logo),
            )
    }

    fn route_button(&self, route: &ShellRoute, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.active_route == Some(route.id);
        let colors = active_theme(cx).tokens().colors;
        let label = SharedString::from(self.translator.read(cx).text(route.label_key).to_owned());
        let tooltip_label = label.clone();
        let route_id = route.id;

        div()
            .id(route.tooltip_id)
            .w(ACTIVITY_RAIL_WIDTH)
            .h(ACTIVITY_TILE_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                activity_bar_button(
                    route.button_id,
                    label,
                    route
                        .icon
                        .clone()
                        .size(ACTIVITY_ICON_SIZE)
                        .text_color(if selected {
                            colors.accent_foreground
                        } else {
                            colors.muted_foreground
                        }),
                    selected,
                    ACTIVITY_BUTTON_SIZE,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.active_route != Some(route_id) {
                        this.active_route = Some(route_id);
                        cx.notify();
                    }
                })),
            )
            .into_any_element()
    }

    fn route_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = active_theme(cx).tokens().spacing.xs;
        v_flex().gap(gap).children(
            self.routes
                .iter()
                .filter(|route| route.visible)
                .map(|route| self.route_button(route, cx)),
        )
    }

    fn action_button(&self, action: &ShellAction, cx: &mut Context<Self>) -> AnyElement {
        let label = SharedString::from(self.translator.read(cx).text(action.label_key).to_owned());
        let tooltip_label = label.clone();
        let handler = Rc::clone(&action.handler);
        let icon = (action.icon)(cx);

        div()
            .id(action.tooltip_id)
            .w(ACTIVITY_RAIL_WIDTH)
            .h(ACTIVITY_TILE_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                activity_bar_button(
                    action.button_id,
                    label,
                    icon.size(ACTIVITY_ICON_SIZE),
                    false,
                    ACTIVITY_BUTTON_SIZE,
                    cx,
                )
                .on_click(move |_, window, cx| handler(window, cx)),
            )
            .into_any_element()
    }

    fn action_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = active_theme(cx).tokens().spacing.xs;
        v_flex().gap(gap).children(
            self.actions
                .iter()
                .filter(|action| action.visible)
                .map(|action| self.action_button(action, cx)),
        )
    }

    /// 是否存在至少一个可见的「可选」路由或动作；活动栏仅在此为真时渲染。
    fn has_visible_optional_item(&self) -> bool {
        self.routes
            .iter()
            .any(|route| route.optional && route.visible)
            || self
                .actions
                .iter()
                .any(|action| action.optional && action.visible)
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.has_visible_optional_item() {
            return None;
        }
        let tokens = active_theme(cx).tokens();
        let colors = tokens.colors;
        let spacing = tokens.spacing;
        Some(
            v_flex()
                .h_full()
                .w(ACTIVITY_RAIL_WIDTH)
                .flex_none()
                .justify_between()
                .bg(colors.surface)
                .border_r_1()
                .border_color(colors.border)
                .pt(spacing.sm)
                .pb(spacing.sm)
                .child(self.route_buttons(cx))
                .child(self.action_buttons(cx)),
        )
    }

    fn active_content(&self) -> AnyElement {
        self.active_route
            .and_then(|active| self.routes.iter().find(|route| route.id == active))
            .map_or_else(
                || div().into_any_element(),
                |route| route.view.clone().into_any_element(),
            )
    }
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .size_full()
            .relative()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .children(self.render_activity_bar(cx))
                    .child(
                        div()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(self.active_content()),
                    ),
            )
            .children(gpui_component::Root::render_sheet_layer(window, cx))
            .children(gpui_component::Root::render_dialog_layer(window, cx))
            .children(gpui_component::Root::render_notification_layer(window, cx))
    }
}
