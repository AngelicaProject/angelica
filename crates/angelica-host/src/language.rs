//! The languages of Angelica's own interface, English and Russian, and the
//! player's choice of one. Plugins' windows show what plugins write.
#![forbid(unsafe_code)]
use crate::manifest::{Class, Permission};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ru")]
    Russian,
}

/// Declares the interface strings once with both translations, so both
/// languages always have the same keys. `{name}`-style placeholders are
/// filled by [`fill`].
macro_rules! strings {
    ($($key:ident: $en:literal, $ru:literal;)*) => {
        /// Angelica's interface strings in one language.
        pub struct Strings {
            $(pub $key: &'static str,)*
        }

        const ENGLISH: Strings = Strings { $($key: $en,)* };
        const RUSSIAN: Strings = Strings { $($key: $ru,)* };

        #[cfg(test)]
        impl Strings {
            pub(crate) fn entries(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($key), self.$key),)*]
            }
        }
    };
}

strings! {
    tab_mine: "My plugins", "Мои плагины";
    tab_settings: "Settings", "Настройки";
    back_to_game: "back to the game", "вернуться в игру";
    hint: "Insert — Angelica", "Insert — Angelica";
    nothing_yet: "No plugins yet", "Плагинов пока нет";
    waiting_one: "{n} waiting for you", "{n} ждёт вас";
    waiting_few: "{n} waiting for you", "{n} ждут вас";
    waiting_many: "{n} waiting for you", "{n} ждут вас";
    running_one: "{n} running", "{n} работает";
    running_few: "{n} running", "{n} работают";
    running_many: "{n} running", "{n} работают";
    plugins_one: "{n} plugin", "{n} плагин";
    plugins_few: "{n} plugins", "{n} плагина";
    plugins_many: "{n} plugins", "{n} плагинов";
    find: "Find among yours", "Найти среди своих";
    group_waiting: "Waiting for you", "Ждут вас";
    group_on: "On", "Включены";
    group_off: "Off", "Выключены";
    row_review: "review", "посмотреть";
    row_stopped: "stopped", "остановлен";
    nothing_found: "Nothing found", "Ничего не нашлось";
    empty: "Your plugins will appear here. Paste a plugin link below.", "Здесь появятся ваши плагины. Вставьте ссылку на плагин ниже.";
    link_hint: "Paste a plugin link", "Вставьте ссылку на плагин";
    link_note: "A plugin from a link comes from its author. What it may do, you decide before it runs.", "Плагин по ссылке приходит от автора. Что ему можно, вы решаете до его запуска.";
    link_invalid: "A plugin link starts with https:// and has no spaces", "Ссылка на плагин начинается с https:// и без пробелов";
    add: "Add", "Добавить";
    adding: "Adding…", "Добавляю…";
    from_catalog: "FROM THE ANGELICA CATALOG", "ИЗ КАТАЛОГА ANGELICA";
    from_author: "FROM ITS AUTHOR", "ОТ АВТОРА";
    source_catalog: "from the Angelica catalog", "из каталога Angelica";
    source_author: "from its author", "от автора";
    asks: "To work, it asks to", "Чтобы работать, он просит";
    asks_more: "The new version also asks to", "Новая версия просит ещё";
    asks_nothing: "Only its own windows and files", "Только свои окна и файлы";
    allow: "Allow and turn on", "Разрешить и включить";
    not_now: "Not now", "Не сейчас";
    open: "Open", "Открыть";
    version: "version", "версия";
    how: "How it works", "Как он устроен";
    front: "Front", "Лицо";
    stopped: "It stopped after an error. The game was not affected.", "Он остановился после ошибки. Игра не пострадала.";
    retry: "Try again", "Попробовать снова";
    keep_off: "Keep it off", "Оставить выключенным";
    back_does: "does", "делает";
    back_own: "its own windows and folder", "свои окна и папку";
    back_cannot: "cannot", "не может";
    cannot_online: "go online", "выходить в интернет";
    cannot_chat: "read your chat", "читать ваш чат";
    cannot_look: "change the game's look", "менять вид игры";
    cannot_always: "touch other plugins or your files", "трогать другие плагины и ваши файлы";
    back_sandbox: "sandbox", "песочница";
    back_frame: "frame", "кадр";
    back_signed: "signed", "подпись";
    back_id: "id", "id";
    setting_language: "Language", "Язык";
    setting_language_note: "Angelica and its windows", "Angelica и её окна";
    setting_key: "Open Angelica", "Открыть Angelica";
    setting_key_note: "The key over the game", "Клавиша поверх игры";
    setting_updates: "Plugins update on their own", "Плагины обновляются сами";
    setting_updates_note: "A version that asks for more waits for you", "Версия, которая просит больше, подождёт вас";
    setting_updates_value: "always", "всегда";
    setting_developers: "For developers", "Для разработчиков";
    setting_developers_note: "Game map, logs, plugin folders", "Карта игры, журналы, папки плагинов";
    show: "Show", "Показать";
    hide: "Hide", "Скрыть";
    dev_map: "Game map", "Карта игры";
    dev_installer: "Plugin installer", "Установщик плагинов";
    dev_diagnostics: "Diagnostics", "Диагностика";
    dev_plugins: "Plugins", "Плагины";
    dev_open_folder: "Open plugin folder", "Открыть папку плагина";
    dev_log: "Log", "Журнал";
    dev_nothing: "Nothing yet", "Пока пусто";
    status_running: "running", "работает";
    status_off: "off", "выключен";
    status_waiting: "waiting for approval", "ждёт разрешения";
    status_stopped: "stopped: {reason}", "остановлен: {reason}";
    note_installed: "{name} is installed. Allow it in Angelica.", "Плагин {name} установлен. Разрешите его в Angelica.";
    note_updated: "{name} is updated. No new permissions.", "Плагин {name} обновлён. Новых разрешений нет.";
    note_updated_asks: "{name} is updated and asks for more. It waits for you.", "Плагин {name} обновлён и просит больше. Он ждёт вас.";
    note_failed: "Could not add the plugin. Details are in For developers.", "Не удалось добавить плагин. Подробности — в «Для разработчиков».";
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Russian];

    /// The language of the system's interface: Russian for Russian,
    /// English otherwise. `language` is a Windows language identifier.
    #[must_use]
    pub fn from_windows(language: u16) -> Self {
        // The primary language is the low 10 bits; LANG_RUSSIAN is 0x19.
        if language & 0x3ff == 0x19 {
            Self::Russian
        } else {
            Self::English
        }
    }

    #[must_use]
    pub fn strings(self) -> &'static Strings {
        match self {
            Self::English => &ENGLISH,
            Self::Russian => &RUSSIAN,
        }
    }

    /// The language's own name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }

    /// The form of a counted word for `n`: one, few, or many.
    fn plural(self, n: usize, [one, few, many]: [&'static str; 3]) -> String {
        let form = match self {
            Self::English if n == 1 => one,
            Self::English => many,
            Self::Russian => match (n % 10, n % 100) {
                (1, tens) if tens != 11 => one,
                (2..=4, tens) if !(12..=14).contains(&tens) => few,
                _ => many,
            },
        };
        fill(form, &[("n", &n.to_string())])
    }

    /// "{n} plugins".
    #[must_use]
    pub fn plugins(self, n: usize) -> String {
        let s = self.strings();
        self.plural(n, [s.plugins_one, s.plugins_few, s.plugins_many])
    }

    /// "{n} waiting for you".
    #[must_use]
    pub fn waiting(self, n: usize) -> String {
        let s = self.strings();
        self.plural(n, [s.waiting_one, s.waiting_few, s.waiting_many])
    }

    /// "{n} running".
    #[must_use]
    pub fn running(self, n: usize) -> String {
        let s = self.strings();
        self.plural(n, [s.running_one, s.running_few, s.running_many])
    }

    /// What the player is told a permission allows.
    #[must_use]
    pub fn permission(self, permission: Permission) -> &'static str {
        match self {
            Self::English => permission.description(),
            Self::Russian => match permission {
                Permission::ExcelText => "Заменяет тексты, которые игра берёт из своих таблиц",
                Permission::TextCase => "Меняет, как игра пишет заглавные буквы",
                Permission::FilesReplace => "Заменяет файлы игры: шрифты и текстуры",
                Permission::ChatCommands => "Добавляет свои команды чата",
                Permission::ChatPrint => "Пишет свои сообщения в ваш чат",
                Permission::HttpGet => "Скачивает с этих сайтов",
                Permission::StateSelf => {
                    "Видит вашего персонажа: имя, класс, уровень, HP и MP, место и зону"
                }
                Permission::ChatRead => "Читает сообщения в вашем чате, в том числе других игроков",
            },
        }
    }

    /// The title of a class of permissions.
    #[must_use]
    pub fn class(self, class: Class) -> &'static str {
        match self {
            Self::English => class.title(),
            Self::Russian => match class {
                Class::Network => "Выходит в интернет",
                Class::OtherPlayers => "Видит других игроков",
                Class::WhatYouSee => "Меняет то, что вы видите",
                Class::YoursOnly => "Только для вас",
            },
        }
    }
}

/// `template` with each `{key}` replaced by its value.
#[must_use]
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_owned();
    for (key, value) in values {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

/// The player's choices for Angelica itself, kept in the installation's
/// state folder (`docs/formats/host-installation-v1.md`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    format: u32,
    /// The language the player chose; the system's without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
}

/// The settings file's name: a plugin id cannot begin with `_`, so it never
/// meets a plugin's settings in the same folder.
const SETTINGS_FILE: &str = "_angelica.json";

impl Settings {
    /// Where the settings are kept in the state folder `state`.
    #[must_use]
    pub fn path(state: &Path) -> PathBuf {
        state.join(SETTINGS_FILE)
    }

    /// The settings at `path`; defaults when there are none or they cannot
    /// be read.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|settings| settings.format == 1)
            .unwrap_or_default()
    }

    /// Writes the settings to `path` through a temporary file and a rename.
    ///
    /// # Errors
    /// The folder or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let settings = Self {
            format: 1,
            ..self.clone()
        };
        let bytes = serde_json::to_vec(&settings).map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(key, _)| key))
            .collect()
    }

    #[test]
    fn both_languages_have_every_string_with_the_same_placeholders() {
        let english = Language::English.strings().entries();
        let russian = Language::Russian.strings().entries();
        assert_eq!(english.len(), russian.len());
        for ((key, en), (other, ru)) in english.into_iter().zip(russian) {
            assert_eq!(key, other);
            assert!(!en.trim().is_empty() && !ru.trim().is_empty(), "{key}");
            assert_eq!(placeholders(en), placeholders(ru), "{key}");
        }
    }

    #[test]
    fn every_permission_and_class_has_both_languages() {
        let permissions = [
            Permission::ExcelText,
            Permission::TextCase,
            Permission::FilesReplace,
            Permission::ChatCommands,
            Permission::ChatPrint,
            Permission::HttpGet,
            Permission::StateSelf,
            Permission::ChatRead,
        ];
        for permission in permissions {
            assert_ne!(
                Language::English.permission(permission),
                Language::Russian.permission(permission)
            );
            assert_ne!(
                Language::English.class(permission.class()),
                Language::Russian.class(permission.class())
            );
        }
    }

    #[test]
    fn russian_counts_take_the_right_form() {
        let ru = Language::Russian;
        assert_eq!(ru.plugins(1), "1 плагин");
        assert_eq!(ru.plugins(3), "3 плагина");
        assert_eq!(ru.plugins(5), "5 плагинов");
        assert_eq!(ru.plugins(11), "11 плагинов");
        assert_eq!(ru.plugins(12), "12 плагинов");
        assert_eq!(ru.plugins(21), "21 плагин");
        assert_eq!(ru.plugins(24), "24 плагина");
        assert_eq!(ru.waiting(1), "1 ждёт вас");
        assert_eq!(ru.running(14), "14 работают");
        assert_eq!(Language::English.plugins(1), "1 plugin");
        assert_eq!(Language::English.plugins(0), "0 plugins");
    }

    #[test]
    fn the_system_language_picks_russian_only_for_russian() {
        assert_eq!(Language::from_windows(0x0419), Language::Russian);
        assert_eq!(Language::from_windows(0x0819), Language::Russian);
        assert_eq!(Language::from_windows(0x0409), Language::English);
        assert_eq!(Language::from_windows(0x0411), Language::English);
    }

    #[test]
    fn settings_round_trip_and_unreadable_ones_are_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let path = Settings::path(folder.path());
        assert_eq!(Settings::load(&path), Settings::default());
        let settings = Settings {
            language: Some(Language::Russian),
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"format":1,"language":"ru"}"#
        );
        assert_eq!(Settings::load(&path).language, Some(Language::Russian));
        std::fs::write(&path, r#"{"format":2,"language":"ru"}"#).unwrap();
        assert_eq!(Settings::load(&path).language, None);
    }
}
