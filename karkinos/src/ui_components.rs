//! Componentes visuales reutilizables del panel de extensiones (T5, S4).
//!
//! Todos los colores salen de los tokens de `Theme`; el único literal es
//! `ERROR_COLOR`, que replica el rojo de diagnósticos de error que ya usa
//! el editor porque la paleta no tiene un token de error propio.

use eframe::egui;

use crate::theme::{lerp_color, Theme};

/// Rojo de diagnósticos de error del editor (`editor_widget.rs`), usado
/// como color "Danger" mientras no exista un token de error en la paleta.
pub const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 90, 90);

const CARD_RADIUS: f32 = 8.0;
const BUTTON_RADIUS: f32 = 6.0;
const CARD_PAD: f32 = 10.0;

/// Tipo de botón tipo píldora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillKind {
    Primary,
    Secondary,
    Danger,
}

/// Estado de una tarjeta de extensión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    Available,
    Downloading,
    Downloaded,
    Active,
    /// Entrada de Showcase: sin acción disponible todavía.
    ComingSoon,
}

/// Contenido de una tarjeta de extensión.
#[derive(Debug, Clone)]
pub struct ExtensionCard {
    pub title: String,
    pub publisher: String,
    pub version: String,
    pub description: String,
    pub downloads: Option<u64>,
    pub state: CardState,
}

/// Acción que pidió el usuario desde una tarjeta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    None,
    Download,
    Delete,
}

/// Formatea una cantidad de descargas de forma compacta (`812`, `34.5K`, `1.2M`).
pub fn format_count(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        scale_tenths(n as f64 / 1_000.0, "K")
    } else {
        scale_tenths(n as f64 / 1_000_000.0, "M")
    }
}

/// Redondea a un decimal con `round()` (medio hacia arriba, no al par) para
/// que `1_250` se muestre como `1.3K` y no como `1.2K`.
fn scale_tenths(value: f64, suffix: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    format!("{rounded:.1}{suffix}")
}

/// Color estable derivado del nombre: mezcla el acento del tema con un tono
/// que sale de un hash FNV-1a del propio nombre.
pub fn avatar_color(theme: &Theme, name: &str) -> egui::Color32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    let tone = egui::Color32::from_rgb(
        (hash & 0xff) as u8,
        ((hash >> 8) & 0xff) as u8,
        ((hash >> 16) & 0xff) as u8,
    );
    lerp_color(theme.accent, tone, 0.55)
}

fn text_size(ui: &egui::Ui, text: &str, font: egui::FontId) -> egui::Vec2 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE).size())
}

fn with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let a = (color.a() as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
}

/// Texto legible sobre `bg` usando solo tokens del tema.
fn readable_text(theme: &Theme, bg: egui::Color32) -> egui::Color32 {
    let lum = 0.2126 * bg.r() as f32 + 0.7152 * bg.g() as f32 + 0.0722 * bg.b() as f32;
    if lum > 140.0 {
        theme.bg
    } else {
        theme.text
    }
}

/// Control segmentado tipo pastilla. Devuelve `true` si cambió la selección.
pub fn segmented_control(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: &mut usize,
    labels: &[&str],
) -> bool {
    let mut changed = false;
    egui::Frame::none()
        .fill(theme.surface_bg)
        .rounding(egui::Rounding::same(BUTTON_RADIUS))
        .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
        .inner_margin(egui::Margin::symmetric(3.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, label) in labels.iter().enumerate() {
                    let font = egui::FontId::proportional(11.0);
                    let size = text_size(ui, label, font.clone()) + egui::vec2(18.0, 8.0);
                    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                    let hover_t =
                        ui.ctx()
                            .animate_bool_with_time(response.id, response.hovered(), 0.12);
                    let active = *selected == i;
                    let bg = if active {
                        theme.selection
                    } else {
                        with_alpha(theme.badge_bg, hover_t)
                    };
                    if bg.a() > 0 {
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::same(4.0), bg);
                    }
                    let color = if active {
                        theme.text
                    } else {
                        lerp_color(theme.text_muted, theme.text, hover_t)
                    };
                    ui.painter()
                        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
                    if response.clicked() && !active {
                        *selected = i;
                        changed = true;
                    }
                    response.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
        });
    changed
}

/// Buscador con ícono y botón de limpiar. Devuelve la respuesta del `TextEdit`
/// para que el llamador detecte Enter (`lost_focus` + `Key::Enter`).
pub fn search_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &mut String,
    hint: &str,
) -> egui::Response {
    let id = ui.make_persistent_id("ui_components_search_field");
    let focused = ui.memory(|m| m.has_focus(id));
    let mut cleared = false;

    let response = egui::Frame::none()
        .fill(theme.bg)
        .rounding(egui::Rounding::same(BUTTON_RADIUS))
        .stroke(egui::Stroke::new(
            1.0_f32,
            if focused {
                theme.border_focus
            } else {
                theme.border_subtle
            },
        ))
        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new("🔍")
                        .size(12.0)
                        .color(theme.text_muted),
                );
                let available = (ui.available_width() - 18.0).max(24.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(text)
                        .id(id)
                        .frame(false)
                        .text_color(theme.text)
                        .hint_text(
                            egui::RichText::new(hint)
                                .size(11.5)
                                .color(theme.text_muted),
                        )
                        .desired_width(available),
                );
                if !text.is_empty() && ui.small_button("✕").on_hover_text("Clear").clicked() {
                    cleared = true;
                }
                edit
            })
            .inner
        })
        .inner;

    if cleared {
        text.clear();
    }
    response
}

/// Círculo con la inicial del nombre y un color estable derivado de él.
pub fn avatar(ui: &mut egui::Ui, theme: &Theme, name: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let color = avatar_color(theme, name);
    ui.painter().circle_filled(rect.center(), size * 0.5, color);
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?');
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(size * 0.45),
        readable_text(theme, color),
    );
}

/// Botón tipo píldora con hover animado.
pub fn pill_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    kind: PillKind,
) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let size = text_size(ui, label, font) + egui::vec2(20.0, 9.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    paint_pill(ui, theme, rect, label, kind, hover_t, 1.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_pill(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    label: &str,
    kind: PillKind,
    hover_t: f32,
    opacity: f32,
) {
    let font = egui::FontId::proportional(11.0);
    let (fill, stroke, text_color) = match kind {
        PillKind::Primary => {
            let base = lerp_color(theme.accent, theme.text, 0.12 * hover_t);
            (
                with_alpha(base, opacity),
                egui::Stroke::NONE,
                with_alpha(readable_text(theme, base), opacity),
            )
        }
        PillKind::Secondary => (
            with_alpha(theme.badge_bg, opacity * hover_t),
            egui::Stroke::new(
                1.0_f32,
                with_alpha(lerp_color(theme.border_subtle, theme.border_focus, hover_t), opacity),
            ),
            with_alpha(lerp_color(theme.text_muted, theme.text, hover_t), opacity),
        ),
        PillKind::Danger => (
            with_alpha(ERROR_COLOR, opacity * 0.12 * hover_t),
            egui::Stroke::new(1.0_f32, with_alpha(ERROR_COLOR, opacity * (0.35 + 0.4 * hover_t))),
            with_alpha(ERROR_COLOR, opacity),
        ),
    };
    if fill.a() > 0 {
        ui.painter()
            .rect_filled(rect, egui::Rounding::same(BUTTON_RADIUS), fill);
    }
    if stroke != egui::Stroke::NONE {
        ui.painter()
            .rect_stroke(rect, egui::Rounding::same(BUTTON_RADIUS), stroke);
    }
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, text_color);
}

fn chip(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    let font = egui::FontId::proportional(10.0);
    let size = text_size(ui, label, font.clone()) + egui::vec2(10.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::Rounding::same(4.0), with_alpha(color, 0.16));
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
}

/// Subtítulo en versalitas pequeñas para separar secciones dentro de una pestaña.
pub fn section_label(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .size(9.5)
            .strong()
            .color(theme.text_muted),
    );
    ui.add_space(2.0);
}

/// Tarjeta de extensión con avatar, estado, descripción de 2 líneas y descargas.
pub fn extension_card(ui: &mut egui::Ui, theme: &Theme, card: &ExtensionCard) -> CardAction {
    let mut action = CardAction::None;
    let height = 112.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);

    let fill = lerp_color(theme.surface_bg, theme.selection, 0.35 * hover_t);
    let border = lerp_color(theme.border_subtle, theme.border_focus, hover_t);
    ui.painter()
        .rect_filled(rect, egui::Rounding::same(CARD_RADIUS), fill);
    ui.painter().rect_stroke(
        rect,
        egui::Rounding::same(CARD_RADIUS),
        egui::Stroke::new(1.0_f32, border),
    );

    let mut content = ui.child_ui(
        rect.shrink(CARD_PAD),
        egui::Layout::top_down(egui::Align::Min),
    );
    content.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

    let controls_w = match card.state {
        CardState::Available => 80.0,
        CardState::Downloading => 100.0,
        CardState::Downloaded | CardState::Active => 118.0,
        CardState::ComingSoon => 82.0,
    };
    let title_w = (content.available_width() - 36.0 - 12.0 - controls_w).max(40.0);

    content.horizontal(|ui| {
        avatar(ui, theme, &card.title, 36.0);
        ui.add_space(2.0);
        ui.allocate_ui_with_layout(
            egui::vec2(title_w, 36.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add_space(1.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&card.title)
                            .size(13.0)
                            .strong()
                            .color(theme.text),
                    )
                    .truncate(true),
                );
                ui.add_space(1.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} · v{}", card.publisher, card.version))
                            .size(10.5)
                            .color(theme.text_muted),
                    )
                    .truncate(true),
                );
            },
        );
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| match card.state {
                CardState::Available => {
                    if pill_button(ui, theme, "Download", PillKind::Primary).clicked() {
                        action = CardAction::Download;
                    }
                }
                CardState::Downloading => {
                    ui.add(egui::Spinner::new().size(13.0));
                    ui.label(
                        egui::RichText::new("Downloading…")
                            .size(10.5)
                            .color(theme.text_muted),
                    );
                }
                CardState::Downloaded | CardState::Active => {
                    let label = if card.state == CardState::Active {
                        "Active"
                    } else {
                        "Downloaded"
                    };
                    chip(ui, label, theme.accent);

                    // Reservamos el lugar del botón para que la fila no salte
                    // cuando aparece en hover; se pinta e interactúa solo cuando
                    // la tarjeta está iluminada.
                    let font = egui::FontId::proportional(11.0);
                    let slot_size = text_size(ui, "Delete", font) + egui::vec2(20.0, 9.0);
                    let (slot, _) = ui.allocate_exact_size(slot_size, egui::Sense::hover());
                    if hover_t > 0.02 {
                        let delete = ui.interact(
                            slot,
                            response.id.with("delete"),
                            egui::Sense::click(),
                        );
                        let delete_t = ui.ctx().animate_bool_with_time(
                            delete.id,
                            delete.hovered(),
                            0.12,
                        );
                        paint_pill(
                            ui,
                            theme,
                            slot,
                            "Delete",
                            PillKind::Danger,
                            delete_t,
                            hover_t,
                        );
                        if delete.clicked() {
                            action = CardAction::Delete;
                        }
                    }
                }
                CardState::ComingSoon => {
                    chip(ui, "Coming soon", theme.text_muted);
                }
            },
        );
    });

    if !card.description.is_empty() {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &card.description,
            0.0,
            egui::text::TextFormat {
                font_id: egui::FontId::proportional(11.0),
                color: theme.text_muted,
                ..Default::default()
            },
        );
        job.wrap.max_width = content.available_width();
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = true;
        content.add(egui::Label::new(job));
    }

    if let Some(downloads) = card.downloads {
        content.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                egui::RichText::new("⬇")
                    .size(10.0)
                    .color(theme.text_muted),
            );
            ui.label(
                egui::RichText::new(format_count(downloads))
                    .size(10.5)
                    .color(theme.text_muted),
            );
        });
    }

    action
}

/// Tarjeta compacta para paquetes `.vsix` ya descargados. Devuelve `true` si
/// el usuario pidió borrar el paquete.
pub fn compact_card(ui: &mut egui::Ui, theme: &Theme, title: &str, subtitle: &str) -> bool {
    let mut delete = false;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 54.0),
        egui::Sense::click(),
    );
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    ui.painter().rect_filled(
        rect,
        egui::Rounding::same(CARD_RADIUS),
        lerp_color(theme.surface_bg, theme.selection, 0.35 * hover_t),
    );
    ui.painter().rect_stroke(
        rect,
        egui::Rounding::same(CARD_RADIUS),
        egui::Stroke::new(
            1.0_f32,
            lerp_color(theme.border_subtle, theme.border_focus, hover_t),
        ),
    );

    let mut content = ui.child_ui(
        rect.shrink(CARD_PAD),
        egui::Layout::top_down(egui::Align::Min),
    );
    content.spacing_mut().item_spacing.y = 2.0;
    content.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .size(11.5)
                        .strong()
                        .color(theme.text),
                )
                .truncate(true),
            );
            ui.label(
                egui::RichText::new(subtitle)
                    .size(10.0)
                    .color(theme.text_muted),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if pill_button(ui, theme, "Delete", PillKind::Danger).clicked() {
                delete = true;
            }
        });
    });
    delete
}

/// Estado vacío centrado con ícono, título y subtítulo.
pub fn empty_state(ui: &mut egui::Ui, theme: &Theme, icon: &str, title: &str, subtitle: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(egui::RichText::new(icon).size(30.0));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(title)
                .size(13.0)
                .strong()
                .color(theme.text),
        );
        ui.add_space(2.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(subtitle)
                    .size(10.5)
                    .color(theme.text_muted),
            )
            .wrap(true),
        );
        ui.add_space(8.0);
    });
}

/// Filas de esqueleto con un brillo que pulsa mientras se espera contenido.
pub fn loading_skeleton(ui: &mut egui::Ui, theme: &Theme, rows: usize) {
    let t = ui.input(|i| i.time) as f32;
    let pulse = 0.5 + 0.5 * (t * 2.5).sin();
    let fill = lerp_color(theme.surface_bg, theme.elevated_bg, 0.35 + 0.5 * pulse);
    for _ in 0..rows {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 68.0),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(rect, egui::Rounding::same(CARD_RADIUS), fill);
        ui.add_space(8.0);
    }
    // Solo repintamos mientras el esqueleto está visible.
    ui.ctx().request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    #[test]
    fn format_count_formatea_descargas() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(812), "812");
        assert_eq!(format_count(1_250), "1.3K");
        assert_eq!(format_count(34_500), "34.5K");
        assert_eq!(format_count(1_200_000), "1.2M");
    }

    #[test]
    fn avatar_color_es_determinista() {
        let theme = ThemeMode::Dark.theme();
        assert_eq!(
            avatar_color(&theme, "rust-analyzer"),
            avatar_color(&theme, "rust-analyzer"),
            "el mismo nombre debe dar el mismo color"
        );
        assert_ne!(
            avatar_color(&theme, "rust-analyzer"),
            avatar_color(&theme, "prettier"),
            "nombres distintos deben dar colores distintos"
        );
    }
}
