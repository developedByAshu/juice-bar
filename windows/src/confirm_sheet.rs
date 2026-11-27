//! The confirm card centred over the settings window before anything destructive (Reset). It stays
//! inside the window, like the Mac's sheet, with Cancel and a red Reset as the Mac app has them.

use crate::canvas::{Align, Canvas};
use crate::theme::Theme;
use crate::util::scale;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::HFONT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB};

// NetSpeedGauge's confirm card, on Mac and Windows alike: the same sizes, words, colours and order.
const CARD_WIDTH: i32 = 400;
const CARD_HEIGHT: i32 = 184;
const CARD_INSET: i32 = 20;
const TILE: i32 = 40;
const BUTTON_HEIGHT: i32 = 30;
const BUTTON_GAP: i32 = 10;
const RADIUS: i32 = 10;
const BUTTON_RADIUS: i32 = 6;
const ICON_RESET: &str = "\u{E72C}";
const DANGER_DARK: u32 = 0xE5484D;
const DANGER_LIGHT: u32 = 0xD93A3F;

/// The red of the Reset button and the card's icon.
pub fn danger_color(theme: &Theme) -> u32 {
    if theme.is_dark { DANGER_DARK } else { DANGER_LIGHT }
}

/// The two buttons on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetButton {
    Confirm,
    Cancel,
}

struct Sheet {
    title: String,
    detail: String,
    confirm_label: String,
    on_confirm: Option<Box<dyn FnOnce()>>,
    hovered: Option<SheetButton>,
    /// Cancel starts focused, so Enter on a destructive card never confirms by accident.
    focused: SheetButton,
}

thread_local! {
    static SHEET: RefCell<Option<Sheet>> = const { RefCell::new(None) };
}

/// The fonts the card draws with, owned by the window it sits in.
pub struct SheetFonts {
    pub title: HFONT,
    pub body: HFONT,
    pub small: HFONT,
    /// The icon font, large, for the reset icon on its tile.
    pub icon: HFONT,
}

/// Opens the card. `on_confirm` runs only when the user picks the confirm button.
pub fn show_confirm(title: &str, detail: &str, on_confirm: impl FnOnce() + 'static) {
    show_confirm_with(title, detail, "Reset", on_confirm);
}

/// Opens the card with its own label on the confirm button.
pub fn show_confirm_with(title: &str, detail: &str, confirm_label: &str, on_confirm: impl FnOnce() + 'static) {
    SHEET.with(|cell| {
        *cell.borrow_mut() = Some(Sheet {
            title: title.to_string(),
            detail: detail.to_string(),
            confirm_label: confirm_label.to_string(),
            on_confirm: Some(Box::new(on_confirm)),
            hovered: None,
            focused: SheetButton::Cancel,
        });
    });
}

/// Returns true while the card is open; the window then sends it every click and key.
pub fn is_confirm_open() -> bool {
    SHEET.with(|cell| cell.borrow().is_some())
}

/// Closes the card without confirming.
pub fn dismiss_confirm() {
    SHEET.with(|cell| cell.borrow_mut().take());
}

/// Splits text into lines that fit a width, breaking between words.
pub fn wrap_text(canvas: &Canvas, text: &str, font: HFONT, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if canvas.text_width(&candidate, font) <= width || line.is_empty() {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        lines.push(line);
    }
    lines
}

/// Returns the card and its two buttons for a window size: Cancel on the left, the red Reset on the right.
fn layout(canvas: &Canvas, fonts: &SheetFonts, dpi: u32, sheet: &Sheet) -> (RECT, RECT, RECT, Vec<String>) {
    let (card_width, card_height) = (scale(CARD_WIDTH, dpi), scale(CARD_HEIGHT, dpi));
    let left = (canvas.width() - card_width) / 2;
    let top = (canvas.height() - card_height) / 2 - scale(20, dpi);
    let card = RECT { left, top, right: left + card_width, bottom: top + card_height };
    let lines = sheet.detail.split('\n').map(str::to_string).collect();
    let button = |label: &str| canvas.text_width(label, fonts.body) + scale(28, dpi);
    let button_top = card.bottom - scale(CARD_INSET, dpi) - scale(BUTTON_HEIGHT, dpi);
    let right = card.right - scale(CARD_INSET, dpi);
    let confirm = RECT { left: right - button(&sheet.confirm_label), top: button_top, right, bottom: button_top + scale(BUTTON_HEIGHT, dpi) };
    let cancel_right = confirm.left - scale(BUTTON_GAP, dpi);
    let cancel = RECT { left: cancel_right - button("Cancel"), top: button_top, right: cancel_right, bottom: confirm.bottom };
    (card, confirm, cancel, lines)
}

/// Draws the dimmed window and the card on top of whatever the window drew.
pub fn paint_confirm(canvas: &mut Canvas, theme: &Theme, fonts: &SheetFonts, dpi: u32, show_focus: bool) {
    SHEET.with(|cell| {
        let sheet = cell.borrow();
        let Some(sheet) = sheet.as_ref() else { return };
        let (width, height) = (canvas.width(), canvas.height());
        let danger = danger_color(theme);
        canvas.round_rect(0, 0, width, height, 0.0, 0x000000, if theme.is_dark { 0.5 } else { 0.25 });
        let (card, confirm, cancel, lines) = layout(canvas, fonts, dpi, sheet);
        let (card_width, card_height) = (card.right - card.left, card.bottom - card.top);
        let radius = scale(RADIUS, dpi) as f32;
        canvas.round_rect(card.left - 1, card.top - 1, card_width + 2, card_height + 2, radius + 1.0, theme.border, 1.0);
        canvas.round_rect(card.left, card.top, card_width, card_height, radius, theme.card, 1.0);
        // The reset icon on a red-tinted tile, then the question and "This cannot be undone" beside it.
        let tile = scale(TILE, dpi);
        let (tile_x, tile_y) = (card.left + scale(CARD_INSET, dpi), card.top + scale(CARD_INSET, dpi));
        canvas.round_rect(tile_x, tile_y, tile, tile, scale(8, dpi) as f32, crate::canvas::mix(theme.card, danger, 0.16), 1.0);
        canvas.text(tile_x + tile / 2, tile_y + (tile - canvas.text_height(fonts.icon)) / 2, ICON_RESET, fonts.icon, danger, Align::Center);
        let text_x = tile_x + tile + scale(12, dpi);
        canvas.text(text_x, tile_y + scale(1, dpi), &sheet.title, fonts.title, theme.text, Align::Left);
        canvas.text(text_x, tile_y + scale(21, dpi), "This cannot be undone", fonts.small, theme.muted, Align::Left);
        for (position, line) in lines.iter().enumerate() {
            canvas.text(card.left + scale(CARD_INSET, dpi), card.top + scale(76 + position as i32 * 20, dpi), line, fonts.body, theme.text, Align::Left);
        }
        for (button, rect, label) in [(SheetButton::Confirm, confirm, sheet.confirm_label.as_str()), (SheetButton::Cancel, cancel, "Cancel")] {
            let is_hovered = sheet.hovered == Some(button);
            let (fill, text) = match button {
                SheetButton::Confirm => (if is_hovered { crate::canvas::mix(danger, 0x000000, 0.12) } else { danger }, 0xFFFFFF),
                SheetButton::Cancel => (if is_hovered { theme.button_hover } else { theme.button }, theme.text),
            };
            canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, scale(BUTTON_RADIUS, dpi) as f32, fill, 1.0);
            if show_focus && sheet.focused == button {
                let ring = scale(2, dpi);
                canvas.round_outline(rect.left - ring - 1, rect.top - ring - 1, rect.right - rect.left + 2 * ring + 2, rect.bottom - rect.top + 2 * ring + 2, 6.0, ring as f32, theme.text, 1.0);
            }
            let text_height = canvas.text_height(fonts.body);
            canvas.text((rect.left + rect.right) / 2, rect.top + (rect.bottom - rect.top - text_height) / 2, label, fonts.body, text, Align::Center);
        }
    });
}

fn button_at(canvas: &Canvas, fonts: &SheetFonts, dpi: u32, x: i32, y: i32) -> Option<SheetButton> {
    SHEET.with(|cell| {
        let sheet = cell.borrow();
        let (_, confirm, cancel, _) = layout(canvas, fonts, dpi, sheet.as_ref()?);
        let inside = |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        if inside(&confirm) {
            Some(SheetButton::Confirm)
        } else if inside(&cancel) {
            Some(SheetButton::Cancel)
        } else {
            None
        }
    })
}

/// Tracks the pointer over the card's buttons. Returns true when the look changed.
pub fn confirm_hover(canvas: &Canvas, fonts: &SheetFonts, dpi: u32, x: i32, y: i32) -> bool {
    let button = button_at(canvas, fonts, dpi, x, y);
    SHEET.with(|cell| {
        let mut sheet = cell.borrow_mut();
        let Some(sheet) = sheet.as_mut() else { return false };
        let changed = sheet.hovered != button;
        sheet.hovered = button;
        changed
    })
}

/// Picks a button: confirm runs the action, either one closes the card.
pub fn press(button: SheetButton) {
    let sheet = SHEET.with(|cell| cell.borrow_mut().take());
    if let (Some(mut sheet), SheetButton::Confirm) = (sheet, button) {
        if let Some(action) = sheet.on_confirm.take() {
            action();
        }
    }
}

/// Handles a click while the card is open. Clicks outside the buttons do nothing, as on a real sheet.
pub fn confirm_click(canvas: &Canvas, fonts: &SheetFonts, dpi: u32, x: i32, y: i32) {
    if let Some(button) = button_at(canvas, fonts, dpi, x, y) {
        press(button);
    }
}

/// Handles a key while the card is open: Tab and arrows move, Enter and Space pick, Esc cancels.
pub fn confirm_key(key: u16) {
    match key {
        VK_ESCAPE => press(SheetButton::Cancel),
        VK_TAB | VK_LEFT | VK_RIGHT => SHEET.with(|cell| {
            if let Some(sheet) = cell.borrow_mut().as_mut() {
                sheet.focused = if sheet.focused == SheetButton::Cancel { SheetButton::Confirm } else { SheetButton::Cancel };
            }
        }),
        VK_RETURN | VK_SPACE => {
            let focused = SHEET.with(|cell| cell.borrow().as_ref().map(|sheet| sheet.focused));
            if let Some(button) = focused {
                press(button);
            }
        }
        _ => {}
    }
}
