//! The saved choices, as plain "key=value" lines like NetSpeedGauge, so a hand edit is easy.

use std::collections::BTreeMap;
use std::path::PathBuf;

const FILE_NAME: &str = "settings.txt";
const TEMP_FILE_NAME: &str = "settings.txt.tmp";

/// What the user chose.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub poll_minutes: u32,
    /// Shows the lowest % next to the icon, not only its colour.
    pub shows_percent_in_icon: bool,
    /// Device id to the name the user typed.
    pub renames: BTreeMap<String, String>,
    /// Device ids left out of the menu.
    pub hidden: Vec<String>,
    /// Device id to the Bluetooth name the user confirmed for it.
    pub confirmed_bluetooth_names: BTreeMap<String, String>,
    /// Off in tests and previews, so they never touch the real file.
    pub is_saved_to_disk: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_minutes: 5,
            shows_percent_in_icon: false,
            renames: BTreeMap::new(),
            hidden: Vec::new(),
            confirmed_bluetooth_names: BTreeMap::new(),
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
        let mut lines = vec![format!("poll_minutes={}", self.poll_minutes), format!("shows_percent_in_icon={}", u8::from(self.shows_percent_in_icon))];
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
