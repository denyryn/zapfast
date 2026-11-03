//! Phone linking with a QR code or pairing code.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke, Vec2};

use crate::app::App;
use crate::backend::LinkStatus;
use crate::model::{Action, Dialog, Provider};
use crate::qr::Qr;
use crate::theme::{self, Icon};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let tagline = match app.provider {
        Provider::Telegram => "A native Telegram client.",
        Provider::WhatsApp => "A native WhatsApp client.",
    };
    card(app, ui, "login", &crate::i18n::gettext(app.locale, tagline), body);
}

/// The centred card of the linking and lock screens: the logo, the name,
/// a line under it, and `body`.
pub(super) fn card(
    app: &mut App,
    ui: &mut egui::Ui,
    salt: &str,
    tagline: &str,
    body: impl FnOnce(&mut App, &mut egui::Ui),
) {
    let palette = app.palette;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(palette.window))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let top = theme::blend(palette.window, palette.accent, 0.10);
            super::widgets::paint_vertical_gradient(ui, rect, top, palette.window);
            let card_width = (460.0_f32.min(rect.width() - 24.0)).max(0.0);
            // Center the card using its previous height. Its content determines
            // the next frame's height.
            let height_id = ui.id().with(format!("{salt}-card-height"));
            let known_height = ui
                .ctx()
                .data(|data| data.get_temp::<f32>(height_id))
                .unwrap_or(560.0);
            let card_height = (known_height.min(rect.height() - 24.0)).max(0.0);
            let card =
                egui::Rect::from_center_size(rect.center(), Vec2::new(card_width, card_height));
            let mut card_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(card)
                    .layout(Layout::top_down(Align::Center)),
            );
            let shown = Frame::new()
                .fill(palette.panel)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS + 8))
                .inner_margin(Margin::same(32))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 16],
                    blur: 48,
                    spread: 0,
                    color: palette.shadow,
                })
                .show(&mut card_ui, |ui| {
                    ui.set_width((card_width - 64.0).max(0.0));
                    ui.spacing_mut().item_spacing.y = 8.0;
                    let (logo, _) = ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::hover());
                    theme::mark(ui, logo.center(), 64.0);
                    ui.add_space(4.0);
                    theme::text(ui, "ZapFast", theme::bold(28.0), palette.text);
                    theme::text(ui, tagline, theme::regular(14.5), palette.secondary);
                    ui.add_space(16.0);
                    body(app, ui);
                });
            let height = shown.response.rect.height();
            if (height - known_height).abs() > 2.0 {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(height_id, height.round()));
                ui.ctx().request_repaint();
            }
        });
}

fn body(app: &mut App, ui: &mut egui::Ui) {
    provider_switch(app, ui);
    match app.provider {
        Provider::Telegram => telegram(app, ui),
        Provider::WhatsApp => whatsapp(app, ui),
    }
    super::accounts::login_choices(app, ui);
    ui.add_space(18.0);
    let note = match app.provider {
        Provider::Telegram => {
            "Unofficial client. Your API ID and hash stay on this computer."
        }
        Provider::WhatsApp => "Unofficial client. Using it may be against WhatsApp's terms of service.",
    };
    theme::paragraph(
        ui,
        crate::i18n::gettext(app.locale, note),
        theme::regular(11.5),
        app.palette.dim,
    );
}

/// The network of the account being linked, chosen beside its tagline: the
/// account is still unlinked, so it can become the other one.
fn provider_switch(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let current = app.provider;
    let entries = [(Provider::WhatsApp, "WhatsApp"), (Provider::Telegram, "Telegram")];
    let mut chosen = None;
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let total = entries
            .iter()
            .map(|(_, label)| theme::soft_button_width(ui, label, false))
            .sum::<f32>()
            + spacing * (entries.len() as f32 - 1.0);
        ui.add_space((ui.available_width() - total).max(0.0) / 2.0);
        for &(provider, label) in &entries {
            let selected = provider == current;
            if theme::soft_button(ui, &palette, None, label, selected).clicked() && !selected {
                chosen = Some(provider);
            }
        }
    });
    ui.add_space(4.0);
    if let Some(provider) = chosen {
        app.actions.push(Action::SwitchProvider(provider));
    }
}

fn whatsapp(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    match app.link.clone() {
        LinkStatus::Starting | LinkStatus::Connecting => {
            busy(
                ui,
                palette.accent,
                &crate::i18n::gettext(locale, "Connecting to WhatsApp…"),
            );
        }
        LinkStatus::Connected | LinkStatus::Disconnected { .. } => {
            busy(
                ui,
                palette.accent,
                &crate::i18n::gettext(locale, "Linked. Waiting for your chats…"),
            );
        }
        LinkStatus::LoggedOut => {
            theme::icon(ui, Icon::Smartphone, 28.0, palette.warning);
            theme::paragraph(
                ui,
                crate::i18n::gettext(
                    locale,
                    "This computer was unlinked from your phone. Requesting a new code.",
                ),
                theme::regular(14.0),
                palette.text,
            );
            ui.add_space(8.0);
            busy(
                ui,
                palette.accent,
                &crate::i18n::gettext(locale, "Requesting a new code…"),
            );
        }
        LinkStatus::Failed(message) => {
            let key_lost = archive_key_lost(&message);
            theme::icon(ui, Icon::CircleAlert, 28.0, palette.danger);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(message)
                        .font(theme::regular(13.5))
                        .color(palette.danger),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if theme::pill_button(
                    ui,
                    &palette,
                    &crate::i18n::gettext(locale, "Try again"),
                    true,
                )
                .clicked()
                {
                    app.actions.push(Action::Reconnect);
                }
                if key_lost
                    && theme::pill_button(
                        ui,
                        &palette,
                        &crate::i18n::gettext(locale, "Start over…"),
                        false,
                    )
                    .clicked()
                {
                    app.actions
                        .push(Action::ShowDialog(crate::model::Dialog::ConfirmStartOver));
                }
            });
        }
        LinkStatus::Unlinked {
            qr,
            pair_code,
            pairing_phone,
        } => {
            if let Some(code) = pair_code {
                pair_code_view(app, ui, &code, pairing_phone.as_deref());
            } else if let Some(phone) = pairing_phone {
                let requesting = crate::i18n::gettext(locale, "Requesting a code for +{phone}…")
                    .replace("{phone}", &phone);
                busy(ui, palette.accent, &requesting);
            } else if let Some(qr) = qr {
                qr_view(app, ui, &qr);
            } else {
                busy(
                    ui,
                    palette.accent,
                    &crate::i18n::gettext(locale, "Waiting for a code from WhatsApp…"),
                );
            }
        }
    }
}

fn telegram(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    match app.tg.auth.clone() {
        crate::tg::Auth::Starting => {
            busy(
                ui,
                palette.accent,
                &crate::i18n::gettext(locale, "Connecting to Telegram…"),
            );
        }
        crate::tg::Auth::Authorized => {
            busy(
                ui,
                palette.accent,
                &crate::i18n::gettext(locale, "Linked. Waiting for your chats…"),
            );
        }
        crate::tg::Auth::Failed(message) => {
            theme::icon(ui, Icon::CircleAlert, 28.0, palette.danger);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(message)
                        .font(theme::regular(13.5))
                        .color(palette.danger),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            if theme::pill_button(
                ui,
                &palette,
                &crate::i18n::gettext(locale, "Try again"),
                true,
            )
            .clicked()
            {
                app.actions.push(Action::Reconnect);
            }
        }
        crate::tg::Auth::NeedCredentials { error } => {
            theme::text(
                ui,
                crate::i18n::gettext(locale, "Connect your Telegram account"),
                theme::semibold(16.0),
                palette.text,
            );
            theme::paragraph(
                ui,
                crate::i18n::gettext(
                    locale,
                    "Telegram gives every client its own API ID and hash. Create them for your account at my.telegram.org and paste them here. They are stored encrypted on this computer and never sent anywhere else.",
                ),
                theme::regular(13.0),
                palette.secondary,
            );
            let api_id = field(ui, &palette, "tg-api-id", &mut app.tg.api_id, "API ID", false);
            let api_hash = field(
                ui,
                &palette,
                "tg-api-hash",
                &mut app.tg.api_hash,
                "API hash",
                false,
            );
            let phone = field(
                ui,
                &palette,
                "tg-phone",
                &mut app.tg.phone,
                "Phone number with country code",
                false,
            );
            auth_error(ui, &palette, error.as_deref());
            let ready = app.tg.api_id.trim().parse::<i32>().is_ok()
                && app.tg.api_hash.trim().len() >= 8
                && app.tg.phone.chars().filter(char::is_ascii_digit).count() >= 7;
            if submit_button(
                ui,
                &palette,
                locale,
                "Send code",
                ready,
                api_id || api_hash || phone,
            ) {
                app.actions.push(Action::TelegramRequestCode);
            }
        }
        crate::tg::Auth::NeedPhone { error } => {
            theme::text(
                ui,
                crate::i18n::gettext(locale, "Enter your phone number"),
                theme::semibold(16.0),
                palette.text,
            );
            theme::paragraph(
                ui,
                crate::i18n::gettext(
                    locale,
                    "Telegram sends a login code to this phone. Use the whole number, starting with the country code.",
                ),
                theme::regular(13.0),
                palette.secondary,
            );
            let phone = field(
                ui,
                &palette,
                "tg-phone",
                &mut app.tg.phone,
                "+15551234567",
                false,
            );
            auth_error(ui, &palette, error.as_deref());
            let ready = app.tg.phone.chars().filter(char::is_ascii_digit).count() >= 7;
            if submit_button(ui, &palette, locale, "Send code", ready, phone) {
                app.actions.push(Action::TelegramRequestCode);
            }
        }
        crate::tg::Auth::NeedCode { error } => {
            theme::text(
                ui,
                crate::i18n::gettext(locale, "Enter the code"),
                theme::semibold(16.0),
                palette.text,
            );
            theme::paragraph(
                ui,
                crate::i18n::gettext(
                    locale,
                    "Telegram sent a login code to your phone. It works once, and only here.",
                ),
                theme::regular(13.0),
                palette.secondary,
            );
            let code = field(
                ui,
                &palette,
                "tg-code",
                &mut app.tg.code,
                "Login code",
                false,
            );
            auth_error(ui, &palette, error.as_deref());
            let ready = app.tg.code.trim().len() >= 4;
            if submit_button(ui, &palette, locale, "Sign in", ready, code) {
                app.actions.push(Action::TelegramSubmitCode);
            }
        }
        crate::tg::Auth::NeedPassword { hint, error } => {
            theme::text(
                ui,
                crate::i18n::gettext(locale, "Two-step verification"),
                theme::semibold(16.0),
                palette.text,
            );
            let message = match &hint {
                Some(hint) => crate::i18n::gettext(locale, "This account asks for its password. Hint: {hint}")
                    .replace("{hint}", hint),
                None => crate::i18n::gettext(
                    locale,
                    "This account asks for its two-step verification password.",
                )
                .into_owned(),
            };
            theme::paragraph(ui, message, theme::regular(13.0), palette.secondary);
            let password = field(
                ui,
                &palette,
                "tg-password",
                &mut app.tg.password,
                "Password",
                true,
            );
            auth_error(ui, &palette, error.as_deref());
            let ready = !app.tg.password.is_empty();
            if submit_button(ui, &palette, locale, "Sign in", ready, password) {
                app.actions.push(Action::TelegramSubmitPassword);
            }
        }
    }
}

fn field(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    id: &str,
    value: &mut String,
    hint: &str,
    secret: bool,
) -> bool {
    let id = egui::Id::new(id);
    let submit = ui.memory(|memory| memory.has_focus(id))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let field = Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_height(
                ui.ctx()
                    .fonts_mut(|fonts| fonts.row_height(&theme::regular(16.0)))
                    + 4.0,
            );
            ui.add(
                egui::TextEdit::singleline(value)
                    .id(id)
                    .hint_text(
                        egui::RichText::new(hint)
                            .color(palette.dim)
                            .font(theme::regular(16.0)),
                    )
                    .font(theme::regular(16.0))
                    .text_color(palette.text)
                    .password(secret)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            );
        });
    theme::focus_outline(ui, id, field.response.rect, f32::from(theme::RADIUS));
    submit
}

fn auth_error(ui: &mut egui::Ui, palette: &theme::Palette, error: Option<&str>) {
    if let Some(error) = error {
        theme::paragraph(ui, error, theme::regular(13.0), palette.danger);
    }
}

fn submit_button(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: crate::i18n::Locale,
    label: &'static str,
    ready: bool,
    submit: bool,
) -> bool {
    let clicked = ui
        .add_enabled_ui(ready, |ui| {
            theme::pill_button(ui, palette, &crate::i18n::gettext(locale, label), true)
        })
        .inner
        .clicked();
    (clicked || submit) && ready
}

pub(super) fn busy(ui: &mut egui::Ui, color: egui::Color32, label: &str) {
    ui.horizontal(|ui| {
        let width = 24.0
            + 8.0
            + ui.painter()
                .layout_no_wrap(label.to_owned(), theme::medium(14.0), color)
                .size()
                .x;
        ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
        theme::spinner(ui, 18.0, color);
        theme::text(ui, label, theme::medium(14.0), ui.visuals().text_color());
    });
}

fn qr_view(app: &mut App, ui: &mut egui::Ui, code: &str) {
    let palette = app.palette;
    let locale = app.locale;
    theme::text(
        ui,
        crate::i18n::gettext(locale, "Link this computer"),
        theme::semibold(16.0),
        palette.text,
    );
    let side = 260.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    match Qr::encode(code) {
        Some(qr) => qr.paint(ui, rect, egui::Color32::BLACK, egui::Color32::WHITE),
        None => {
            ui.painter().rect_filled(rect, 8.0, palette.surface);
            theme::paint_icon(ui, Icon::CircleAlert, rect, 32.0, palette.danger);
        }
    }
    ui.add_space(4.0);
    let step1 = crate::i18n::gettext(locale, "Open WhatsApp on your phone");
    let step2 = crate::i18n::gettext(locale, "Tap Menu or Settings, then Linked devices");
    let step3 = crate::i18n::gettext(locale, "Tap Link a device and point the phone at this code");
    let steps = [&step1, &step2, &step3];
    for (index, step) in steps.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            theme::text(
                ui,
                format!("{}.", index + 1),
                theme::semibold(13.0),
                palette.accent,
            );
            theme::text(ui, step.as_ref(), theme::regular(13.0), palette.secondary);
        });
    }
    ui.add_space(10.0);
    if theme::link(
        ui,
        crate::i18n::gettext(locale, "Link with phone number instead"),
        theme::medium(13.0),
        palette.link,
    )
    .clicked()
    {
        app.actions.push(Action::ShowDialog(Dialog::PairWithPhone));
    }
}

fn pair_code_view(app: &mut App, ui: &mut egui::Ui, code: &str, phone: Option<&str>) {
    let palette = app.palette;
    let locale = app.locale;
    theme::text(
        ui,
        crate::i18n::gettext(locale, "Enter this code on your phone"),
        theme::semibold(16.0),
        palette.text,
    );
    if let Some(phone) = phone {
        let for_phone = crate::i18n::gettext(locale, "for +{phone}").replace("{phone}", phone);
        theme::text(ui, for_phone, theme::regular(13.0), palette.secondary);
    }
    ui.add_space(8.0);
    let shown = if code.len() == 8 && !code.contains('-') {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_owned()
    };
    Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(22, 12))
        .show(ui, |ui| {
            theme::text(ui, &shown, theme::bold(30.0), palette.text);
        });
    ui.add_space(8.0);
    let step1 = crate::i18n::gettext(locale, "Open WhatsApp on your phone");
    let step2 = crate::i18n::gettext(locale, "Tap Menu or Settings, then Linked devices");
    let step3 = crate::i18n::gettext(
        locale,
        "Tap Link a device, then Link with phone number instead",
    );
    let steps = [&step1, &step2, &step3];
    for (index, step) in steps.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            theme::text(
                ui,
                format!("{}.", index + 1),
                theme::semibold(13.0),
                palette.accent,
            );
            theme::text(ui, step.as_ref(), theme::regular(13.0), palette.secondary);
        });
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - 200.0).max(0.0) / 2.0);
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::Copy),
            &crate::i18n::gettext(locale, "Copy code"),
            false,
        )
        .clicked()
        {
            app.actions.push(Action::CopyText(code.to_owned()));
        }
    });
}

/// Whether the archive's key is gone for good, rather than the keyring being
/// locked or unavailable, which "Try again" can fix.
fn archive_key_lost(message: &str) -> bool {
    message.contains("OS keyring key is missing")
        || message.contains("archive key in the OS keyring is invalid")
        || message.contains("could not be unlocked with its OS keyring key")
}

#[cfg(test)]
mod start_over_tests {
    #[test]
    fn only_a_lost_key_offers_to_start_over() {
        for lost in [
            "The archive is encrypted but its OS keyring key is missing. Restore the original keyring; the archive has not been changed",
            "The archive key in the OS keyring is invalid",
            "The archive could not be unlocked with its OS keyring key: file is not a database",
        ] {
            assert!(super::archive_key_lost(lost), "{lost}");
        }
        for recoverable in [
            "Unlock your OS keyring and restart ZapFast",
            "The OS keyring could not open ZapFast's archive key",
        ] {
            assert!(!super::archive_key_lost(recoverable), "{recoverable}");
        }
    }
}
