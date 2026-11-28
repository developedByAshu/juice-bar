//! The saved choices, as plain "key=value" lines like NetSpeedGauge, so a hand edit is easy.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::update::UpdateCheck;

const FILE_NAME: &str = "settings.txt";
const TEMP_FILE_NAME: &str = "settings.txt.tmp";

/// The menu's text size in points, as NetSpeedGauge's font size: the whole menu scales from it.
pub const MENU_FONT_POINTS_DEFAULT: u32 = 9;
pub const MENU_FONT_POINTS_MIN: u32 = 7;
pub const MENU_FONT_POINTS_MAX: u32 = 12;
/// The window theme's last choice, dark.
pub const MAX_WINDOW_THEME: u8 = 2;

/// What resting the pointer on the icon does, as NetSpeedGauge's "On hover".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoverAction {
    /// Only the plain tooltip.
    #[default]
    Nothing,
    /// A small card with each device's %.
    Card,
}

/// What a left click on the icon does, as NetSpeedGauge's "On click". A right click always opens
/// Settings and Quit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClickAction {
    /// The connected devices: the header and one row per device.
    #[default]
    Devices,
    Settings,
}

impl HoverAction {
    pub fn from_number(number: u8) -> Self {
        if number == 1 { Self::Card } else { Self::Nothing }
    }

    pub fn number(self) -> u8 {
        self as u8
    }
}

impl ClickAction {
    /// Settings is stored as 2, its number before the menu and card became one devices flyout; the old
    /// 0 (menu) and 1 (card) both load as Devices.
    pub fn from_number(number: u8) -> Self {
        if number == 2 { Self::Settings } else { Self::Devices }
    }

    pub fn number(self) -> u8 {
        match self {
            Self::Devices => 0,
            Self::Settings => 2,
        }
    }
}

/// What the user chose.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub poll_minutes: u32,
    /// Shows the lowest % next to the icon, not only its colour.
    pub shows_percent_in_icon: bool,
    /// The menu's text size, 7 to 12 points. Its rows, icons, gaps and width scale with it.
    pub menu_font_points: u32,
    pub hover_action: HoverAction,
    pub click_action: ClickAction,
    /// Device id to the name the user typed.
    pub renames: BTreeMap<String, String>,
    /// Device ids left out of the menu.
    pub hidden: Vec<String>,
    /// Device id to the Bluetooth name the user confirmed for it.
    pub confirmed_bluetooth_names: BTreeMap<String, String>,
    /// Juice Bar's own windows: 0 same as the system, 1 light, 2 dark, as NetSpeedGauge's window_theme.
    pub window_theme: u8,
    /// Settings stays above other apps.
    pub keeps_on_top: bool,
    /// How often to look for a new release.
    pub update_check: UpdateCheck,
    /// When the last look happened, in seconds since 1970. Reset all keeps it.
    pub last_update_check: u64,
    /// Off in tests and previews, so they never touch the real file.
    pub is_saved_to_disk: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_minutes: 5,
            shows_percent_in_icon: false,
            menu_font_points: MENU_FONT_POINTS_DEFAULT,
            hover_action: HoverAction::Nothing,
            click_action: ClickAction::Devices,
            renames: BTreeMap::new(),
            hidden: Vec::new(),
            confirmed_bluetooth_names: BTreeMap::new(),
            window_theme: 0,
            keeps_on_top: false,
            update_check: UpdateCheck::Daily,
            last_update_check: 0,
            is_saved_to_disk: true,
        }
    }
}

/// Returns the folder that holds the settings: Application Support on Mac, AppData on Windows.
pub fn data_folder() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("JuiceBar")
    } else {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Library/Application Support/JuiceBar")
    }
}

impl Settings {
    /// Returns the settings as text, one choice per line.
    pub fn to_text(&self) -> String {
        let mut lines = vec![
            format!("poll_minutes={}", self.poll_minutes),
            format!("shows_percent_in_icon={}", u8::from(self.shows_percent_in_icon)),
            format!("menu_font_points={}", self.menu_font_points),
            format!("hover_action={}", self.hover_action.number()),
            format!("click_action={}", self.click_action.number()),
            format!("window_theme={}", self.window_theme),
            format!("keep_on_top={}", u8::from(self.keeps_on_top)),
            format!("update_check={}", self.update_check.number()),
            format!("last_update_check={}", self.last_update_check),
        ];
        lines.extend(self.renames.iter().map(|(id, name)| format!("rename.{id}={name}")));
        lines.extend(self.hidden.iter().map(|id| format!("hidden.{id}=1")));
        lines.extend(self.confirmed_bluetooth_names.iter().map(|(id, name)| format!("bluetooth_name.{id}={name}")));
        lines.join("\n") + "\n"
    }

    /// Reads settings from text. Unknown or broken lines are skipped, so an old file still loads.
    pub fn from_text(text: &str) -> Self {
        let mut settings = Self::default();
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            let (key, value) = (key.trim(), value.trim());
            match key {
                "poll_minutes" => settings.poll_minutes = value.parse().unwrap_or(5).clamp(1, 60),
                "shows_percent_in_icon" => settings.shows_percent_in_icon = value == "1",
                "menu_font_points" => {
                    settings.menu_font_points = value.parse().unwrap_or(MENU_FONT_POINTS_DEFAULT).clamp(MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_MAX);
                }
                "hover_action" => settings.hover_action = HoverAction::from_number(value.parse().unwrap_or(0)),
                "click_action" => settings.click_action = ClickAction::from_number(value.parse().unwrap_or(0)),
                "window_theme" => settings.window_theme = value.parse::<u8>().unwrap_or(0).min(MAX_WINDOW_THEME),
                "keep_on_top" => settings.keeps_on_top = value == "1",
                "update_check" => settings.update_check = UpdateCheck::from_number(value.parse().unwrap_or(UpdateCheck::Daily.number())),
                "last_update_check" => settings.last_update_check = value.parse().unwrap_or(0),
                _ => {
                    if let Some(id) = key.strip_prefix("rename.") {
                        settings.renames.insert(id.to_string(), value.to_string());
                    } else if let Some(id) = key.strip_prefix("hidden.") {
                        settings.hidden.push(id.to_string());
                    } else if let Some(id) = key.strip_prefix("bluetooth_name.") {
                        settings.confirmed_bluetooth_names.insert(id.to_string(), value.to_string());
                    }
                }
            }
        }
        settings
    }

    /// Puts the Appearance page's choices back to their defaults, for its "Reset this page" row.
    pub fn reset_appearance(&mut self) {
        let defaults = Self::default();
        self.shows_percent_in_icon = defaults.shows_percent_in_icon;
        self.menu_font_points = defaults.menu_font_points;
        self.window_theme = defaults.window_theme;
    }

    /// Puts every choice back to its default, keeping when updates were last checked.
    pub fn reset_all(&mut self) {
        let is_saved_to_disk = self.is_saved_to_disk;
        *self = Self { last_update_check: self.last_update_check, is_saved_to_disk, ..Self::default() };
    }

    /// Loads the saved choices, or the defaults when there are none yet.
    pub fn load() -> Self {
        std::fs::read_to_string(data_folder().join(FILE_NAME)).map_or_else(|_| Self::default(), |text| Self::from_text(&text))
    }

    /// Saves through a temporary file, so a crash cannot damage the choices. Previews skip this.
    pub fn save(&self) {
        if !self.is_saved_to_disk {
            return;
        }
        let folder = data_folder();
        let temp_path = folder.join(TEMP_FILE_NAME);
        if std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&temp_path, self.to_text())).is_ok() {
            let _ = std::fs::rename(&temp_path, folder.join(FILE_NAME));
        }
    }
}
