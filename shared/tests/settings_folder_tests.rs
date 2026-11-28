//! The settings folder needs a full home path. Its own test file, so changing HOME races no other test.

use juicebar_shared::settings::data_folder;

#[test]
fn no_home_or_a_relative_one_gives_no_settings_folder() {
    let variable = if cfg!(windows) { "APPDATA" } else { "HOME" };
    let saved = std::env::var_os(variable);
    std::env::remove_var(variable);
    assert_eq!(data_folder(), None, "unset");
    std::env::set_var(variable, "relative/home");
    assert_eq!(data_folder(), None, "relative");
    std::env::set_var(variable, if cfg!(windows) { "C:\\Users\\test\\AppData\\Roaming" } else { "/Users/test" });
    assert!(data_folder().is_some_and(|folder| folder.is_absolute() && folder.ends_with("JuiceBar")));
    match saved {
        Some(value) => std::env::set_var(variable, value),
        None => std::env::remove_var(variable),
    }
}
