//! The confirm card that slides over the settings window before anything destructive (Reset). It
//! stays inside the window, like a sheet on Mac, instead of a separate system dialog box.

use crate::canvas::{Align, Canvas};
use crate::theme::Theme;
use crate::util::scale;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::HFONT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB};

const CARD_WIDTH: i32 = 380;
const CARD_PADDING: i32 = 24;
const BUTTON_HEIGHT: i32 = 32;
const BUTTON_WIDTH: i32 = 112;
const BUTTON_GAP: i32 = 8;
const RADIUS: f32 = 8.0;
const LINE_GAP: i32 = 4;

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

/// Returns the card and its two buttons for a window size.
fn layout(canvas: &Canvas, fonts: &SheetFonts, dpi: u32, sheet: &Sheet) -> (RECT, RECT, RECT, Vec<String>) {
    let card_width = scale(CARD_WIDTH, dpi).min(canvas.width() - scale(32, dpi));
    let padding = scale(CARD_PADDING, dpi);
    let lines = wrap_text(canvas, &sheet.detail, fonts.body, card_width - 2 * padding);
    let title_height = canvas.text_height(fonts.title);
    let line_height = canvas.text_height(fonts.body) + scale(LINE_GAP, dpi);
    let card_height = padding + title_height + scale(12, dpi) + lines.len() as i32 * line_height + scale(20, dpi) + scale(BUTTON_HEIGHT, dpi) + padding;
    let left = (canvas.width() - card_width) / 2;
    let top = (canvas.height() - card_height) / 2;
    let card = RECT { left, top, right: left + card_width, bottom: top + card_height };
    let button_width = scale(BUTTON_WIDTH, dpi);
    let button_top = card.bottom - padding - scale(BUTTON_HEIGHT, dpi);
    let cancel = RECT { left: card.right - padding - button_width, top: button_top, right: card.right - padding, bottom: button_top + scale(BUTTON_HEIGHT, dpi) };
    let confirm = RECT { left: cancel.left - scale(BUTTON_GAP, dpi) - button_width, top: button_top, right: cancel.left - scale(BUTTON_GAP, dpi), bottom: cancel.bottom };
    (card, confirm, cancel, lines)
}

/// Draws the dimmed window and the card on top of whatever the window drew.
pub fn paint_confirm(canvas: &mut Canvas, theme: &Theme, fonts: &SheetFonts, dpi: u32, show_focus: bool) {
    SHEET.with(|cell| {
        let sheet = cell.borrow();
        let Some(sheet) = sheet.as_ref() else { return };
        let (width, height) = (canvas.width(), canvas.height());
        canvas.round_rect(0, 0, width, height, 0.0, 0x000000, if theme.is_dark { 0.45 } else { 0.3 });
        let (card, confirm, cancel, lines) = layout(canvas, fonts, dpi, sheet);
        let padding = scale(CARD_PADDING, dpi);
        canvas.round_rect(card.left, card.top, card.right - card.left, card.bottom - card.top, RADIUS, theme.card, 1.0);
        canvas.round_outline(card.left, card.top, card.right - card.left, card.bottom - card.top, RADIUS, 1.0, theme.border, 1.0);
        canvas.text(card.left + padding, card.top + padding, &sheet.title, fonts.title, theme.text, Align::Left);
        let mut y = card.top + padding + canvas.text_height(fonts.title) + scale(12, dpi);
        for line in &lines {
            canvas.text(card.left + padding, y, line, fonts.body, theme.muted, Align::Left);
            y += canvas.text_height(fonts.body) + scale(LINE_GAP, dpi);
        }
        for (button, rect, label) in [(SheetButton::Confirm, confirm, sheet.confirm_label.as_str()), (SheetButton::Cancel, cancel, "Cancel")] {
            let is_hovered = sheet.hovered == Some(button);
            let (fill, text) = match button {
                SheetButton::Confirm => (theme.accent, theme.on_accent),
                SheetButton::Cancel => (if theme.is_dark { 0x3A3A3A } else { 0xFBFBFB }, theme.text),
            };
            let fill = if is_hovered { crate::canvas::mix(fill, if theme.is_dark { 0xFFFFFF } else { 0x000000 }, 0.08) } else { fill };
            canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 4.0, fill, 1.0);
            if button == SheetButton::Cancel {
                canvas.round_outline(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 4.0, 1.0, theme.border, 1.0);
            }
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
