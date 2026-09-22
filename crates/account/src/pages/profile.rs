//! 已登录 profile 卡片：头像 + 昵称/套餐徽标 + Origin ID 胶囊 + 退出登录。
//! 视觉规则逐条对齐 Flutter `settings_page.dart` 的 `_profileBody` /
//! `_NicknameRowState._planPill` / `_PlanTag`：
//! - 头像：强调色 12% 圆底 + 强调色首字符，无首字符回退 lucide `cloud`。
//! - 套餐徽标：只由 `CloudPlan.badge` 决定是否显示；颜色取 `badge_color`（回退强调色）；
//!   样式 outline / solid / medal / ribbon / plain；`badge_numbered` 时追加 `No.0001`。
//! - Origin ID：强调色 12% 底 + 强调色等宽数字 + 复制图标；无 ID 灰色 `#—` 不可点。

use fluxdown_protocol::{AgentSessionDto, CloudPlan};
use fluxdown_ui_components::{ButtonVariant, button, card, icon_button};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::SemanticThemeTokens;
use gpui::{
    ClickEvent, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
    white,
};
use gpui_component::{
    Icon, IconName, Sizable as _, StyledExt as _, h_flex, spinner::Spinner, tooltip::Tooltip,
    v_flex,
};

use crate::assets::{CLOUD_ICON_PATH, CROWN_ICON_PATH, REFRESH_ICON_PATH};
use crate::view::AccountView;
use crate::{AccountCommand, t};

/// 与 Flutter 的 `withValues(alpha: 0.12)` 一致的浅底透明度。
const TINT_ALPHA: f32 = 0.12;

/// 昵称首字符大写；无可用字符返回 `None`（头像回退图标）。
fn avatar_initial(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let first = trimmed.chars().next()?;
    Some(first.to_uppercase().collect())
}

/// profile 卡片的瞬时 UI 状态；与会话数据分开传，避免参数逐个铺开。
pub(crate) struct ProfileState {
    /// Origin ID 刚被复制，胶囊显示「已复制」。
    pub copied: bool,
    /// 本机服务失联，所有动作置灰。
    pub disabled: bool,
    /// 「刷新云端信息」在途。
    pub refreshing: bool,
    /// 卡片底部常驻错误行。
    pub last_error: Option<SharedString>,
}

pub(crate) fn render(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    session: &AgentSessionDto,
    state: ProfileState,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let ProfileState {
        copied: origin_id_copied,
        disabled,
        refreshing,
        last_error,
    } = state;
    let display_name = if session.user.nickname.trim().is_empty() {
        session
            .user
            .email
            .split('@')
            .next()
            .unwrap_or(&session.user.email)
            .to_owned()
    } else {
        session.user.nickname.clone()
    };
    let initial = avatar_initial(&display_name);
    let plan_badge = session
        .current_plan
        .as_ref()
        .and_then(|plan| plan_badge(tokens, plan, session.user.membership_ordinal));
    let accent = tokens.colors.accent_foreground;
    let origin_id = session.user.origin_id;
    let logout_label = t(translator, "accountLogout");
    let refresh_label = t(translator, "accountCloudRefresh");
    let refresh_tooltip = refresh_label.clone();
    let copied_label = t(translator, "accountOriginIdCopied");

    let row = h_flex()
        .w_full()
        .items_center()
        .gap(tokens.spacing.md)
        .child(
            div()
                .size(px(46.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens.radius.full)
                .bg(accent.opacity(TINT_ALPHA))
                .map(|this| match &initial {
                    Some(initial) => this.child(
                        div()
                            .text_size(px(19.))
                            .font_semibold()
                            .text_color(accent)
                            .child(initial.clone()),
                    ),
                    None => this.child(
                        Icon::empty()
                            .path(CLOUD_ICON_PATH)
                            .size(px(22.))
                            .text_color(accent),
                    ),
                }),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .gap(px(6.))
                .child(
                    h_flex()
                        .gap(tokens.spacing.sm)
                        .items_center()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_semibold()
                                .truncate()
                                .text_color(tokens.colors.foreground)
                                .child(display_name),
                        )
                        .when_some(plan_badge, |this, badge| this.child(badge)),
                )
                .child(origin_id_pill(
                    tokens,
                    origin_id,
                    origin_id_copied,
                    copied_label,
                    cx,
                )),
        )
        .child(
            h_flex()
                .flex_none()
                .items_center()
                .gap(tokens.spacing.xs)
                .child(
                    icon_button(
                        "account-cloud-refresh",
                        refresh_label,
                        if refreshing {
                            Spinner::new()
                                .with_size(px(14.))
                                .icon(Icon::empty().path(REFRESH_ICON_PATH))
                                .color(tokens.colors.accent_foreground)
                                .into_any_element()
                        } else {
                            Icon::empty()
                                .path(REFRESH_ICON_PATH)
                                .size(px(14.))
                                .text_color(tokens.colors.muted_foreground)
                                .into_any_element()
                        },
                        ButtonVariant::Ghost,
                        cx,
                    )
                    .disabled(disabled || refreshing)
                    .tooltip(move |window, cx| {
                        Tooltip::new(refresh_tooltip.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(
                        move |view, _: &ClickEvent, window: &mut gpui::Window, cx| {
                            view.refresh_cloud_info(window, cx);
                        },
                    )),
                )
                .child(
                    button("account-logout", logout_label, ButtonVariant::Secondary, cx)
                        .h(px(28.))
                        .px(tokens.spacing.sm)
                        .text_size(px(12.5))
                        .disabled(disabled)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            let future = view.controller.port().execute(AccountCommand::Auth {
                                method: fluxdown_protocol::method::AGENT_AUTH_LOGOUT,
                                params: serde_json::json!({}),
                            });
                            view.spawn_action(future, cx);
                        })),
                ),
        );

    card(cx)
        .w_full()
        .p(tokens.spacing.lg)
        .flex()
        .flex_col()
        .gap(tokens.spacing.md)
        .child(row)
        .when_some(last_error, |this, error| {
            this.child(
                div()
                    .text_size(px(11.5))
                    .text_color(tokens.colors.destructive)
                    .child(error),
            )
        })
}

/// 套餐徽标：`badge` 为空则不渲染（免费/专业版等服务端未配置徽标的套餐）。
fn plan_badge(
    tokens: &SemanticThemeTokens,
    plan: &CloudPlan,
    membership_ordinal: Option<i64>,
) -> Option<gpui::AnyElement> {
    let badge = plan.badge.as_deref().filter(|badge| !badge.is_empty())?;
    let color = parse_hex_color(&plan.badge_color).unwrap_or(tokens.colors.accent_foreground);
    let text = with_membership_ordinal(
        badge,
        plan.badge_numbered.then_some(membership_ordinal).flatten(),
        plan.badge_number_digits,
    );
    Some(plan_tag(text, color, &plan.badge_style))
}

/// Flutter `_withMembershipOrdinal`：`{badge} No.{ordinal 补零到 digits(1..=6)}`。
fn with_membership_ordinal(base: &str, ordinal: Option<i64>, digits: i64) -> SharedString {
    match ordinal {
        Some(ordinal) => {
            let width = usize::try_from(digits.clamp(1, 6)).unwrap_or(1);
            SharedString::from(format!("{base} No.{ordinal:0width$}"))
        }
        None => SharedString::from(base.to_owned()),
    }
}

/// Flutter `_PlanTag`：outline | solid | medal | ribbon | plain，全部纯色/描边，不用渐变。
fn plan_tag(text: SharedString, color: Hsla, style: &str) -> gpui::AnyElement {
    let crown = |size: f32, tint: Hsla| {
        Icon::empty()
            .path(CROWN_ICON_PATH)
            .size(px(size))
            .text_color(tint)
    };
    match style {
        "outline" => h_flex()
            .items_center()
            .gap(px(3.))
            .px(px(7.))
            .py(px(1.5))
            .rounded_full()
            .border_1()
            .border_color(color)
            .bg(color.opacity(0.08))
            .child(crown(9., color))
            .child(
                div()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(text),
            )
            .into_any_element(),
        "solid" => div()
            .px(px(7.))
            .py(px(1.5))
            .rounded_full()
            .bg(color)
            .text_size(px(9.5))
            .font_weight(FontWeight::BOLD)
            .text_color(white())
            .child(text)
            .into_any_element(),
        "medal" => h_flex()
            .items_center()
            .overflow_hidden()
            .rounded_full()
            .border_1()
            .border_color(color)
            .child(
                div()
                    .px(px(5.))
                    .py(px(1.5))
                    .bg(color)
                    .child(crown(9., white())),
            )
            .child(
                div()
                    .px(px(6.))
                    .py(px(1.5))
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(text),
            )
            .into_any_element(),
        "ribbon" => div()
            .px(px(7.))
            .py(px(1.5))
            .rounded_full()
            .bg(color)
            .text_size(px(9.5))
            .font_weight(FontWeight::EXTRA_BOLD)
            .text_color(white())
            .child(text)
            .into_any_element(),
        _ => div()
            .px(px(6.))
            .py(px(1.))
            .rounded_full()
            .bg(color.opacity(TINT_ALPHA))
            .text_size(px(9.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .child(text)
            .into_any_element(),
    }
}

/// Flutter `_tryParseHexColor`：接受可选 `#` 前缀的 6 位 `RRGGBB` 或 8 位 `AARRGGBB`。
fn parse_hex_color(value: &str) -> Option<Hsla> {
    let hex = value.trim().trim_start_matches('#');
    let parsed = u32::from_str_radix(hex, 16).ok()?;
    let (argb, alpha) = match hex.len() {
        6 => (parsed, 1.),
        8 => (parsed & 0x00FF_FFFF, f32::from((parsed >> 24) as u8) / 255.),
        _ => return None,
    };
    let mut color: Hsla = gpui::rgb(argb).into();
    color.a = alpha;
    Some(color)
}

fn origin_id_pill(
    tokens: &SemanticThemeTokens,
    origin_id: Option<i64>,
    copied: bool,
    copied_label: SharedString,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let color = if origin_id.is_some() {
        tokens.colors.accent_foreground
    } else {
        tokens.colors.muted_foreground
    };
    let label = if copied {
        copied_label
    } else {
        match origin_id {
            Some(id) => SharedString::from(format!("#{id}")),
            None => SharedString::from("#—"),
        }
    };
    let pill = h_flex()
        .id("account-origin-id")
        .items_center()
        .px(px(8.))
        .py(px(3.))
        .gap(px(3.))
        .rounded_full()
        .bg(color.opacity(TINT_ALPHA))
        .text_size(px(11.5))
        .font_semibold()
        .text_color(color)
        .child(label);
    if let Some(origin_id) = origin_id {
        pill.cursor_pointer()
            .child(Icon::new(IconName::Copy).size(px(10.)).text_color(color))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.copy_origin_id(origin_id, cx);
            }))
    } else {
        pill
    }
}
