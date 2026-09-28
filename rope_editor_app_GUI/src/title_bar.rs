use eframe::egui;

pub enum TitleBarIcon {
    Close,
    Minimize,
    Maximize,
    Restore,
}

/// Dibuja un botón de la barra de título con un ícono hecho a mano
/// (líneas/rectángulos), en vez de un emoji — los emojis dependen de
/// qué fuente tenga el sistema, y bajo WSLg a veces se ven mal o
/// distinto a como se ven en Windows de verdad.
pub fn title_bar_icon_button(ui: &mut egui::Ui, icon: TitleBarIcon) -> egui::Response {
    let size = egui::vec2(32.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if response.hovered() {
        let bg = if matches!(icon, TitleBarIcon::Close) {
            egui::Color32::from_rgb(196, 43, 28) // hover rojo, como en Windows
        } else {
            egui::Color32::from_white_alpha(25)
        };
        ui.painter().rect_filled(rect, 0.0, bg);
    }

    let stroke = egui::Stroke::new(1.2, egui::Color32::from_gray(225));
    let center = rect.center();
    let painter = ui.painter();

    match icon {
        TitleBarIcon::Close => {
            let h = 4.5;
            painter.line_segment([center + egui::vec2(-h, -h), center + egui::vec2(h, h)], stroke);
            painter.line_segment([center + egui::vec2(-h, h), center + egui::vec2(h, -h)], stroke);
        }
        TitleBarIcon::Minimize => {
            painter.line_segment(
                [center + egui::vec2(-5.0, 5.0), center + egui::vec2(5.0, 5.0)],
                stroke,
            );
        }
        TitleBarIcon::Maximize => {
            let square = egui::Rect::from_center_size(center, egui::vec2(9.0, 9.0));
            painter.rect_stroke(square, 0.0, stroke);
        }
        TitleBarIcon::Restore => {
            // El ícono clásico de "restaurar": dos ventanas superpuestas.
            let back = egui::Rect::from_center_size(center + egui::vec2(2.0, -2.0), egui::vec2(8.0, 8.0));
            let front = egui::Rect::from_center_size(center + egui::vec2(-1.5, 1.5), egui::vec2(8.0, 8.0));
            painter.rect_stroke(back, 0.0, stroke);
            // Tapamos la parte de atrás que "taparía" la ventana de
            // adelante, para que se vea como dos ventanas de verdad
            // superpuestas y no como dos cuadrados cruzados.
            painter.rect_filled(front, 0.0, egui::Color32::from_rgb(18, 18, 18));
            painter.rect_stroke(front, 0.0, stroke);
        }
    }

    response
}

/// Bordes invisibles en los 4 lados + 4 esquinas de la ventana, para
/// poder seguir redimensionando arrastrando el borde con el mouse
/// ahora que le sacamos la decoración nativa (que era la que hacía
/// esto automáticamente antes).
pub fn draw_resize_handles(ctx: &egui::Context) {
    const BORDER: f32 = 6.0;
    const CORNER: f32 = 12.0;

    let rect = ctx.input(|i| i.screen_rect());

    let handles: [(&str, egui::Rect, egui::CursorIcon, egui::ResizeDirection); 8] = [
        (
            "resize_n",
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), BORDER)),
            egui::CursorIcon::ResizeNorth,
            egui::ResizeDirection::North,
        ),
        (
            "resize_s",
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - BORDER),
                egui::vec2(rect.width(), BORDER),
            ),
            egui::CursorIcon::ResizeSouth,
            egui::ResizeDirection::South,
        ),
        (
            "resize_w",
            egui::Rect::from_min_size(rect.min, egui::vec2(BORDER, rect.height())),
            egui::CursorIcon::ResizeWest,
            egui::ResizeDirection::West,
        ),
        (
            "resize_e",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - BORDER, rect.min.y),
                egui::vec2(BORDER, rect.height()),
            ),
            egui::CursorIcon::ResizeEast,
            egui::ResizeDirection::East,
        ),
        (
            "resize_nw",
            egui::Rect::from_min_size(rect.min, egui::vec2(CORNER, CORNER)),
            egui::CursorIcon::ResizeNorthWest,
            egui::ResizeDirection::NorthWest,
        ),
        (
            "resize_ne",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - CORNER, rect.min.y),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeNorthEast,
            egui::ResizeDirection::NorthEast,
        ),
        (
            "resize_sw",
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeSouthWest,
            egui::ResizeDirection::SouthWest,
        ),
        (
            "resize_se",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - CORNER, rect.max.y - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeSouthEast,
            egui::ResizeDirection::SouthEast,
        ),
    ];

    for (id, handle_rect, cursor, direction) in handles {
        egui::Area::new(egui::Id::new(id))
            .fixed_pos(handle_rect.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(handle_rect, egui::Sense::drag());
                if response.hovered() {
                    ctx.set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}