//! 设置页的自有布局原语：分类页 → 子 Tab → 分组卡片 → 设置行。
//!
//! 与 Flutter 桌面端 `lib/src/pages/settings_page.dart` 同一视觉语言：
//! 左侧分类导航、顶部「标题 + 描述 + 子 Tab」头部、白色内容区、
//! 宽视口双列瀑布式分组卡片。控件全部由 gpui-component 公开原语渲染。

use std::rc::Rc;

use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    Anchor, AnyElement, App, AppContext as _, ElementId, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _, Size,
    button::Button,
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
    v_flex,
};

/// 触发双列排布的最小内容宽度（与 Flutter `_AdaptiveSections` 一致）。
pub(crate) const TWO_COLUMN_MIN_WIDTH: f32 = 920.;

type Getter<T> = Rc<dyn Fn(&App) -> T>;
type Setter<T> = Rc<dyn Fn(T, &mut App)>;
/// 自定义渲染：`(disabled, key, window, cx)`。
type Renderer = Rc<dyn Fn(bool, &SharedString, &mut Window, &mut App) -> AnyElement>;

/// 一行设置的右侧控件。
#[derive(Clone)]
pub(crate) enum Control {
    Switch {
        get: Getter<bool>,
        set: Setter<bool>,
    },
    Number {
        min: f64,
        max: f64,
        step: f64,
        get: Getter<f64>,
        set: Setter<f64>,
    },
    Input {
        get: Getter<SharedString>,
        set: Setter<SharedString>,
    },
    Dropdown {
        options: Vec<(SharedString, SharedString)>,
        get: Getter<SharedString>,
        set: Setter<SharedString>,
    },
    Custom(Renderer),
}

impl Control {
    pub(crate) fn switch<G, S>(get: G, set: S) -> Self
    where
        G: Fn(&App) -> bool + 'static,
        S: Fn(bool, &mut App) + 'static,
    {
        Self::Switch {
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    pub(crate) fn number<G, S>(min: f64, max: f64, step: f64, get: G, set: S) -> Self
    where
        G: Fn(&App) -> f64 + 'static,
        S: Fn(f64, &mut App) + 'static,
    {
        Self::Number {
            min,
            max,
            step,
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    pub(crate) fn input<G, S>(get: G, set: S) -> Self
    where
        G: Fn(&App) -> SharedString + 'static,
        S: Fn(SharedString, &mut App) + 'static,
    {
        Self::Input {
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    pub(crate) fn dropdown<G, S>(options: Vec<(SharedString, SharedString)>, get: G, set: S) -> Self
    where
        G: Fn(&App) -> SharedString + 'static,
        S: Fn(SharedString, &mut App) + 'static,
    {
        Self::Dropdown {
            options,
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    /// 自定义控件；闭包收到禁用态与本行的稳定状态键。
    pub(crate) fn custom<F, E>(render: F) -> Self
    where
        E: IntoElement,
        F: Fn(bool, &SharedString, &mut Window, &mut App) -> E + 'static,
    {
        Self::Custom(Rc::new(move |disabled, key, window, cx| {
            render(disabled, key, window, cx).into_any_element()
        }))
    }
}

/// 分组卡片内的一行。
#[derive(Clone)]
pub(crate) struct SettingsRow {
    pub(crate) title: SharedString,
    pub(crate) description: Option<SharedString>,
    keywords: Vec<SharedString>,
    disabled: bool,
    vertical: bool,
    /// `None` 表示整行自渲染（[`Self::custom`]）。
    control: Option<Control>,
    full: Option<Renderer>,
    /// 显式高度权重；双列切分用。
    weight: Option<f32>,
}

impl SettingsRow {
    pub(crate) fn new(title: impl Into<SharedString>, control: Control) -> Self {
        Self {
            title: title.into(),
            description: None,
            keywords: Vec::new(),
            disabled: false,
            vertical: false,
            control: Some(control),
            full: None,
            weight: None,
        }
    }

    /// 整行自渲染（列表、按钮组等无「标题 + 控件」结构的行）。
    pub(crate) fn custom<F, E>(render: F) -> Self
    where
        E: IntoElement,
        F: Fn(bool, &SharedString, &mut Window, &mut App) -> E + 'static,
    {
        Self {
            title: SharedString::default(),
            description: None,
            keywords: Vec::new(),
            disabled: false,
            vertical: false,
            control: None,
            full: Some(Rc::new(move |disabled, key, window, cx| {
                render(disabled, key, window, cx).into_any_element()
            })),
            weight: None,
        }
    }

    #[must_use]
    pub(crate) fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub(crate) fn keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SharedString>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 控件换行到标题下方（长输入、编辑器、选择器）。
    #[must_use]
    pub(crate) fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let hit = |text: &SharedString| text.to_lowercase().contains(query);
        hit(&self.title)
            || self.description.as_ref().is_some_and(hit)
            || self.keywords.iter().any(hit)
    }

    /// Flutter `_AdaptiveSections._weightOf` 的行权重。
    fn layout_weight(&self) -> f32 {
        self.weight.unwrap_or(if self.full.is_some() {
            3.0
        } else if self.vertical {
            2.4
        } else {
            1.0
        })
    }

    fn render(
        &self,
        key: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Stateful<gpui::Div> {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let disabled = self.disabled;
        let mut row = div()
            .id(ElementId::from(key.clone()))
            .w_full()
            .px(px(16.))
            .py(if self.vertical { px(12.) } else { px(10.) })
            .when(disabled, |this| this.opacity(0.5));

        if let Some(full) = self.full.clone() {
            return row.child(full(disabled, &key, window, cx));
        }

        let control = self
            .control
            .clone()
            .map(|control| render_control(&control, disabled, &key, self.vertical, window, cx));
        let label = v_flex()
            .gap(px(2.))
            .min_w_0()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child(self.title.clone()),
            )
            .when_some(self.description.clone(), |this, description| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(colors.muted_foreground)
                        .child(description),
                )
            });

        if self.vertical {
            row = row.child(
                v_flex()
                    .w_full()
                    .gap(px(10.))
                    .child(label)
                    .children(control.map(|control| div().w_full().child(control))),
            );
        } else {
            row = row.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    // 标题列保底 160px；控件列可收缩，避免宽控件把描述挤成一列。
                    .child(div().flex_1().min_w(px(160.)).child(label))
                    .children(control.map(|control| div().min_w_0().child(control))),
            );
        }
        row
    }
}

fn render_control(
    control: &Control,
    disabled: bool,
    key: &SharedString,
    vertical: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match control {
        Control::Custom(render) => render(disabled, key, window, cx),
        Control::Switch { get, set } => {
            let checked = get(cx);
            let set = set.clone();
            Switch::new(ElementId::from(SharedString::from(format!("{key}-switch"))))
                .checked(checked)
                .disabled(disabled)
                .on_click(move |checked: &bool, _, cx| set(*checked, cx))
                .into_any_element()
        }
        Control::Number {
            min,
            max,
            step,
            get,
            set,
        } => render_number(
            *min, *max, *step, get, set, disabled, key, vertical, window, cx,
        ),
        Control::Input { get, set } => render_input(get, set, disabled, key, vertical, window, cx),
        Control::Dropdown { options, get, set } => {
            let current = get(cx);
            let label = options
                .iter()
                .find(|(value, _)| *value == current)
                .map_or_else(|| current.clone(), |(_, label)| label.clone());
            let options = options.clone();
            let set = set.clone();
            Button::new(ElementId::from(SharedString::from(format!(
                "{key}-dropdown"
            ))))
            .outline()
            // small 取 text_sm 字号（Medium 会用 text_base，字号偏大），
            // 高度另行拉到与 Input / NumberInput 一致的 28px。
            .small()
            .h(CONTROL_HEIGHT)
            .label(label)
            .dropdown_caret(true)
            .disabled(disabled)
            .when(vertical, |this| this.w_full())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let current = current.clone();
                let set = set.clone();
                options.iter().fold(menu, |menu, (value, label)| {
                    let checked = *value == current;
                    let value = value.clone();
                    let set = set.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(checked)
                            .on_click(move |_, _, cx| set(value.clone(), cx)),
                    )
                })
            })
            .into_any_element()
        }
    }
}

struct InputSlot {
    state: gpui::Entity<InputState>,
    _subscriptions: Vec<gpui::Subscription>,
}

fn render_input(
    get: &Getter<SharedString>,
    set: &Setter<SharedString>,
    disabled: bool,
    key: &SharedString,
    vertical: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let value = get(cx);
    let slot = window.use_keyed_state(
        ElementId::from(SharedString::from(format!("{key}-input"))),
        cx,
        {
            let value = value.clone();
            let set = set.clone();
            move |window, cx| {
                let state = cx.new(|cx| InputState::new(window, cx).default_value(value));
                let subscription = cx.subscribe(&state, move |_, state, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        set(state.read(cx).value(), cx);
                    }
                });
                InputSlot {
                    state,
                    _subscriptions: vec![subscription],
                }
            }
        },
    );
    slot.update(cx, |slot, cx| {
        if slot.state.read(cx).value() != value {
            slot.state.update(cx, |state, cx| {
                state.set_value(value.clone(), window, cx);
            });
        }
    });
    let state = slot.read(cx).state.clone();
    Input::new(&state)
        .with_size(Size::Medium)
        .disabled(disabled)
        .map(|this| {
            if vertical {
                this.w_full()
            } else {
                this.w(px(240.))
            }
        })
        .into_any_element()
}

struct NumberSlot {
    state: gpui::Entity<InputState>,
    current: f64,
    _subscriptions: Vec<gpui::Subscription>,
}

#[allow(clippy::too_many_arguments)]
fn render_number(
    min: f64,
    max: f64,
    step: f64,
    get: &Getter<f64>,
    set: &Setter<f64>,
    disabled: bool,
    key: &SharedString,
    vertical: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let value = get(cx);
    let slot = window.use_keyed_state(
        ElementId::from(SharedString::from(format!("{key}-number"))),
        cx,
        {
            let set = set.clone();
            let step_set = set.clone();
            move |window, cx| {
                let state =
                    cx.new(|cx| InputState::new(window, cx).default_value(format_number(value)));
                let subscriptions = vec![
                    cx.subscribe_in(&state, window, {
                        move |slot: &mut NumberSlot, state, event: &NumberInputEvent, window, cx| {
                            let NumberInputEvent::Step(action) = event;
                            let Ok(current) = state.read(cx).value().parse::<f64>() else {
                                return;
                            };
                            let next = if *action == StepAction::Increment {
                                current + step
                            } else {
                                current - step
                            }
                            .clamp(min, max);
                            state.update(cx, |state, cx| {
                                state.set_value(
                                    SharedString::from(format_number(next)),
                                    window,
                                    cx,
                                );
                            });
                            slot.current = next;
                            step_set(next, cx);
                        }
                    }),
                    cx.subscribe_in(&state, window, {
                        move |slot: &mut NumberSlot, state, event: &InputEvent, window, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let text = state.read(cx).value();
                            let Ok(parsed) = text.parse::<f64>() else {
                                return;
                            };
                            let clamped = parsed.clamp(min, max);
                            if (clamped - slot.current).abs() < f64::EPSILON {
                                return;
                            }
                            slot.current = clamped;
                            set(clamped, cx);
                            if (clamped - parsed).abs() >= f64::EPSILON {
                                state.update(cx, |state, cx| {
                                    state.set_value(
                                        SharedString::from(format_number(clamped)),
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }
                    }),
                ];
                NumberSlot {
                    state,
                    current: value,
                    _subscriptions: subscriptions,
                }
            }
        },
    );
    slot.update(cx, |slot, cx| {
        if (slot.current - value).abs() >= f64::EPSILON {
            slot.current = value;
            slot.state.update(cx, |state, cx| {
                state.set_value(SharedString::from(format_number(value)), window, cx);
            });
        }
    });
    let state = slot.read(cx).state.clone();
    NumberInput::new(&state)
        .with_size(Size::Medium)
        .disabled(disabled)
        .map(|this| {
            if vertical {
                this.w_full()
            } else {
                this.w(px(132.))
            }
        })
        .into_any_element()
}

/// 整数不显示小数位（`3` 而非 `3.0`）。
fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// 一组设置：可选小节标题 + 一张卡片（行间发丝线）。
#[derive(Clone)]
pub(crate) struct SettingsSection {
    title: Option<SharedString>,
    subtitle: Option<SharedString>,
    rows: Vec<SettingsRow>,
}

impl SettingsSection {
    pub(crate) fn new() -> Self {
        Self {
            title: None,
            subtitle: None,
            rows: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub(crate) fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    #[must_use]
    pub(crate) fn row(mut self, row: SettingsRow) -> Self {
        self.rows.push(row);
        self
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn filtered(&self, query: &str) -> Option<Self> {
        let title_hit = !query.is_empty()
            && (self
                .title
                .as_ref()
                .is_some_and(|title| title.to_lowercase().contains(query))
                || self
                    .subtitle
                    .as_ref()
                    .is_some_and(|text| text.to_lowercase().contains(query)));
        let rows: Vec<SettingsRow> = if title_hit {
            self.rows.clone()
        } else {
            self.rows
                .iter()
                .filter(|row| row.matches(query))
                .cloned()
                .collect()
        };
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            rows,
        })
    }

    fn layout_weight(&self) -> f32 {
        let heading = if self.title.is_some() { 0.6 } else { 0. };
        heading
            + self
                .rows
                .iter()
                .map(SettingsRow::layout_weight)
                .sum::<f32>()
    }

    fn render(
        &self,
        key: &str,
        index: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let hairline = colors.border.opacity(0.6);
        let mut card = v_flex()
            .w_full()
            .bg(colors.surface)
            .border_1()
            .border_color(colors.border.opacity(0.75))
            .rounded(px(10.))
            .overflow_hidden();
        for (row_index, row) in self.rows.iter().enumerate() {
            if row_index > 0 {
                card = card.child(
                    div()
                        .w_full()
                        .h(px(1.))
                        .pl(px(16.))
                        .child(div().size_full().bg(hairline)),
                );
            }
            card = card.child(row.render(
                SharedString::from(format!("{key}-{index}-{row_index}")),
                window,
                cx,
            ));
        }

        let heading = (self.title.is_some() || self.subtitle.is_some()).then(|| {
            v_flex()
                .pl(px(4.))
                .pb(px(6.))
                .gap(px(2.))
                .children(self.title.clone().map(|title| {
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.muted_foreground)
                        .child(title)
                }))
                .children(self.subtitle.clone().map(|subtitle| {
                    div()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground.opacity(0.85))
                        .child(subtitle)
                }))
        });

        v_flex().w_full().children(heading).child(card)
    }
}

/// 分类页内的一个子 Tab。
#[derive(Clone)]
pub(crate) struct SettingsTab {
    pub(crate) id: &'static str,
    pub(crate) label: SharedString,
    sections: Vec<SettingsSection>,
    /// 由 capability 注入的整页视图（账户 / 扩展）。
    view: Option<gpui::AnyView>,
}

impl SettingsTab {
    pub(crate) fn new(id: &'static str, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            sections: Vec::new(),
            view: None,
        }
    }

    #[must_use]
    pub(crate) fn section(mut self, section: SettingsSection) -> Self {
        if !section.is_empty() {
            self.sections.push(section);
        }
        self
    }

    #[must_use]
    pub(crate) fn sections<I: IntoIterator<Item = SettingsSection>>(mut self, sections: I) -> Self {
        for section in sections {
            self = self.section(section);
        }
        self
    }

    #[must_use]
    pub(crate) fn view(mut self, view: gpui::AnyView) -> Self {
        self.view = Some(view);
        self
    }

    fn filtered(&self, query: &str) -> Option<Self> {
        if self.view.is_some() {
            return (query.is_empty() || self.label.to_lowercase().contains(query))
                .then(|| self.clone());
        }
        let sections: Vec<SettingsSection> = self
            .sections
            .iter()
            .filter_map(|section| section.filtered(query))
            .collect();
        if sections.is_empty() {
            return None;
        }
        Some(Self {
            id: self.id,
            label: self.label.clone(),
            sections,
            view: None,
        })
    }

    fn render(&self, key: &str, width: f32, window: &mut Window, cx: &mut App) -> AnyElement {
        if let Some(view) = self.view.clone() {
            return div().w_full().child(view).into_any_element();
        }
        // 单分组 Tab 里分组标题与 Tab 标签同名时省略标题，避免视觉重复。
        let mut sections = self.sections.clone();
        if let [section] = sections.as_mut_slice()
            && section.title.as_ref() == Some(&self.label)
        {
            section.title = None;
        }

        let gap = px(16.);
        // 列数由宿主测得的内容宽度决定：不能用 container_query（它把高度钉死成
        // 父级高度，内容无法撑开，滚动容器就永远没有可滚区）。
        if sections.len() < 2 || width < TWO_COLUMN_MIN_WIDTH {
            let mut column = v_flex().w_full().gap(gap);
            for (index, section) in sections.iter().enumerate() {
                column = column.child(section.render(key, index, window, cx));
            }
            return column.into_any_element();
        }

        let weights: Vec<f32> = sections
            .iter()
            .map(SettingsSection::layout_weight)
            .collect();
        let total: f32 = weights.iter().sum();
        let mut accumulated = 0.;
        let mut split = 0usize;
        while split + 1 < sections.len() && accumulated + weights[split] / 2. < total / 2. {
            accumulated += weights[split];
            split += 1;
        }
        let split = split.max(1);
        let mut left = v_flex().flex_1().min_w_0().gap(gap);
        let mut right = v_flex().flex_1().min_w_0().gap(gap);
        for (index, section) in sections.iter().enumerate() {
            let child = section.render(key, index, window, cx);
            if index < split {
                left = left.child(child);
            } else {
                right = right.child(child);
            }
        }
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(px(24.))
            .child(left)
            .child(right)
            .into_any_element()
    }
}

/// 一个设置分类页。
pub(crate) struct SettingsPage {
    pub(crate) key: &'static str,
    pub(crate) title: SharedString,
    pub(crate) description: SharedString,
    pub(crate) icon: IconName,
    tabs: Vec<SettingsTab>,
}

impl SettingsPage {
    pub(crate) fn new(
        key: &'static str,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        icon: IconName,
    ) -> Self {
        Self {
            key,
            title: title.into(),
            description: description.into(),
            icon,
            tabs: Vec::new(),
        }
    }

    /// 无子 Tab 的单页分类：全部分组归入隐式默认 Tab。
    #[must_use]
    pub(crate) fn sections<I: IntoIterator<Item = SettingsSection>>(self, sections: I) -> Self {
        self.tab(SettingsTab::new("", SharedString::default()).sections(sections))
    }

    #[must_use]
    pub(crate) fn tab(mut self, tab: SettingsTab) -> Self {
        self.tabs.push(tab);
        self
    }

    pub(crate) fn nav_icon(&self) -> Icon {
        Icon::new(self.icon.clone())
    }

    /// 按查询过滤；无命中返回 `None`（分类在搜索结果中隐藏）。
    pub(crate) fn filtered(self, query: &str) -> Option<Self> {
        if query.is_empty() {
            return Some(self);
        }
        let tabs: Vec<SettingsTab> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.filtered(query))
            .collect();
        if tabs.is_empty() {
            return None;
        }
        Some(Self { tabs, ..self })
    }

    /// 可见子 Tab（单页分类返回空切片）。
    pub(crate) fn visible_tabs(&self) -> &[SettingsTab] {
        if self.tabs.len() < 2 { &[] } else { &self.tabs }
    }

    /// 渲染指定子 Tab 的内容；未知 id 回退首个 Tab。
    pub(crate) fn render_tab(
        &self,
        tab_id: &str,
        width: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .or_else(|| self.tabs.first());
        match tab {
            Some(tab) => tab.render(&format!("{}-{}", self.key, tab.id), width, window, cx),
            None => div().into_any_element(),
        }
    }
}
