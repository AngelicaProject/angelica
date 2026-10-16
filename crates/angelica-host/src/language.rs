//! The languages of Angelica's own interface, English and Russian, and the
//! player's choice of one. Plugins' windows show what plugins write.
#![forbid(unsafe_code)]
use crate::{
    key::MenuKey,
    manifest::{Class, Permission},
    theme::{Size, Theme},
};
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
    tab_installed: "Installed", "Установленные";
    tab_catalog: "Catalog", "Каталог";
    catalog_loading: "Loading the catalog…", "Загрузка каталога…";
    catalog_failed: "The catalog did not load: {reason}", "Каталог не загрузился: {reason}";
    catalog_empty: "The catalog has no plugins yet.", "В каталоге пока нет плагинов.";
    catalog_install: "Install", "Установить";
    catalog_installed: "Installed", "Установлен";
    catalog_withdrawn: "{name} is turned off: the catalog withdrew it. {reason}", "{name} выключен: каталог отозвал его. {reason}";
    tab_settings: "Settings", "Настройки";
    close: "Close", "Закрыть";
    word_on: "On", "Вкл";
    bar_back: "Back", "Назад";
    bar_collapse: "Collapse", "Свернуть";
    bar_expand: "Expand", "Развернуть";
    bar_menu: "Menu", "Меню";
    menu_check_updates: "Check for updates", "Проверить обновления";
    arrange_panels: "Arrange panels", "Расставить панели";
    arrange_hint: "Drag panels where you want them. Right-click one to choose when it shows.", "Перетащите панели, куда удобно. Правый клик — когда показывать.";
    arrange_done: "Done", "Готово";
    shown_always: "Always", "Всегда";
    shown_in_combat: "In combat", "В бою";
    shown_out_of_combat: "Out of combat", "Вне боя";
    shown_hidden: "Hidden", "Скрыта";
    word_off: "Off", "Выкл";
    esc_closes: "Esc closes", "Esc — закрыть";
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
    find: "Search", "Поиск";
    filter_all: "All permissions", "Все права";
    column_plugin: "Plugin", "Плагин";
    column_permissions: "Permissions", "Права";
    column_version: "Version", "Версия";
    group_attention: "Needs attention", "Требуют внимания";
    group_on: "On", "Включены";
    group_off: "Off", "Выключены";
    nothing_found: "Nothing found", "Ничего не нашлось";
    empty: "Your plugins will appear here. Paste a plugin link below.", "Здесь появятся ваши плагины. Вставьте ссылку на плагин ниже.";
    link_hint: "Plugin link", "Ссылка на плагин";
    link_note: "A plugin from a link comes from its author. What it may do, you decide before it runs.", "Плагин по ссылке приходит от автора. Что ему можно, вы решаете до его запуска.";
    link_invalid: "A plugin link starts with https:// and has no spaces", "Ссылка на плагин начинается с https:// и без пробелов";
    add: "+ Add", "+ Добавить";
    adding: "Adding…", "Добавляю…";
    source_catalog: "from the Angelica catalog", "из каталога Angelica";
    source_development: "in development, built on this computer", "в разработке, собран на этом компьютере";
    lens_ungranted: "Wants to read {domain}, which the game map does not offer: not allowed", "Хочет читать {domain}, чего карта игры не даёт: не разрешено";
    lens_act_ungranted: "Wants to act in {domain}, which the game map does not offer: not allowed", "Хочет действовать в {domain}, чего карта игры не даёт: не разрешено";
    lens_change_ungranted: "Wants to change {domain}, which the game map does not offer: not allowed", "Хочет менять {domain}, чего карта игры не даёт: не разрешено";
    acts_warning: "Acting for you in the game breaks its terms of service: the game's company can suspend your account for it.", "Действия за вас в игре нарушают её пользовательское соглашение: компания игры может за это заблокировать аккаунт.";
    source_author: "from its author", "от автора";
    source_authors: "from {authors}", "от {authors}";
    version: "version {version}", "версия {version}";
    dialog_new: "Turning on", "Включение";
    dialog_more: "New permissions", "Новые права";
    asks: "Will be able to", "Сможет";
    asks_first: "Asks for the first time", "Просит впервые";
    already: "Already allowed", "Уже разрешено";
    asks_nothing: "Only its own windows and files", "Только свои окна и файлы";
    allow: "Allow and turn on", "Разрешить и включить";
    not_now: "Not now", "Не сейчас";
    review: "Review permissions…", "Посмотреть права…";
    open: "Open", "Открыть";
    turn_off: "Turn off", "Выключить";
    turn_on: "Turn on", "Включить";
    retry: "Try again", "Попробовать снова";
    keep_off: "Keep it off", "Оставить выключенным";
    open_folder: "Open folder", "Открыть папку";
    copy_report: "Copy report", "Скопировать отчёт";
    report_copied: "Report copied", "Отчёт скопирован";
    remove: "Remove…", "Удалить…";
    remove_title: "Removing", "Удаление";
    remove_keep: "Remove, keep its data", "Удалить, данные оставить";
    remove_all: "Remove with its data", "Удалить вместе с данными";
    remove_note: "Its data are its settings and the files it saved. Kept data come back if you install it again.", "Данные плагина — его настройки и сохранённые файлы. Оставленные данные вернутся, если установить его снова.";
    cancel: "Cancel", "Отмена";
    history: "This session", "В этой сессии";
    history_installed: "installed, version {version}", "установлен, версия {version}";
    history_updated: "updated from {from} to {to}", "обновлён с {from} до {to}";
    history_compiled: "compiled again for this version of Angelica", "собран заново для этой версии Angelica";
    history_update_failed: "could not update: {reason}", "не удалось обновить: {reason}";
    history_compile_failed: "could not compile again: {reason}", "не удалось собрать заново: {reason}";
    state_running: "Running", "Работает";
    state_off: "Off", "Выключен";
    state_waiting: "Waiting for your permission", "Ждёт вашего разрешения";
    state_stopped: "Stopped after an error. The game was not affected.", "Остановлен после ошибки. Игра не пострадала.";
    section_general: "General", "Общие";
    section_updates: "Updates", "Обновления";
    section_developers: "For developers", "Для разработчиков";
    setting_key: "Menu key", "Клавиша меню";
    setting_stop_key: "Stop plugins acting", "Остановить действия плагинов";
    stop_key_note: "Stops every plugin from acting in the game at once, and lets them act again. The same keys as the menu's; Esc keeps {key}.", "Сразу запрещает всем плагинам действовать в игре и снова разрешает. Те же клавиши, что для меню; Esc оставит {key}.";
    note_acting: "{name} is acting in the game", "{name} действует в игре";
    note_stopped: "Plugins are stopped from acting", "Плагинам запрещено действовать";
    hint_stop: "{key} stops it", "{key} — остановить";
    hint_resume: "{key} lets them act again", "{key} — снова разрешить";
    key_press: "Press a key…", "Нажмите клавишу…";
    key_note: "Insert, Delete, Home, End, Page Up, Page Down, Pause, Scroll Lock, or F1 to F12. Esc keeps {key}.", "Insert, Delete, Home, End, Page Up, Page Down, Pause, Scroll Lock или F1–F12. Esc оставит {key}.";
    setting_language: "Language", "Язык";
    setting_theme: "Theme", "Тема";
    theme_light: "light", "светлая";
    theme_dark: "dark", "тёмная";
    setting_size: "Interface size", "Размер интерфейса";
    size_small: "small", "маленький";
    size_normal: "normal", "обычный";
    size_large: "large", "крупный";
    setting_angelica: "Angelica", "Angelica";
    setting_channel: "Channel", "Канал";
    setting_map: "Game map", "Карта игры";
    map_ready: "updated to {version} in this session", "обновлена до {version} в этой игре";
    channel_stable: "stable", "стабильный";
    channel_nightly: "nightly", "ночной";
    channel_note: "Nightly builds come after every change, for testers; they may stop the game.", "Ночные сборки выходят после каждого изменения, для тестеров; они могут остановить игру.";
    angelica_version: "version {version}", "версия {version}";
    angelica_development: "a development build, which does not update", "сборка для разработки, она не обновляется";
    angelica_not_checked: "not checked yet", "ещё не проверялась";
    angelica_checking: "checking…", "проверяем…";
    angelica_up_to_date: "the newest", "самая новая";
    angelica_ready: "{version} starts with the next game start", "{version} запустится при следующем запуске игры";
    angelica_failed: "could not check: {reason}", "не удалось проверить: {reason}";
    setting_check: "Plugins", "Плагины";
    check_now: "Check now", "Проверить сейчас";
    checking: "Checking…", "Проверяю…";
    checked_never: "not checked yet", "ещё не проверялись";
    checked_now: "checked just now", "проверено только что";
    checked_minutes: "checked {n} min ago", "проверено {n} мин назад";
    checked_hours: "checked {n} h ago", "проверено {n} ч назад";
    check_note: "Updates are checked at the game's start and every 6 hours and installed automatically. A version that needs new permissions waits for your approval.", "Обновления проверяются при запуске игры и каждые 6 часов и ставятся автоматически. Версия, которой нужны новые разрешения, ждёт вашего согласия.";
    dev_mode: "Dev mode", "Режим разработки";
    dev_mode_note: "For work on the game map: angelica look reads the running game, and a map from angelica map try takes the signed map's place. Plugins can do nothing more than before.", "Для работы над картой игры: angelica look читает запущенную игру, а карта из angelica map try заменяет подписанную. Плагины не получают ничего сверх того, что могли.";
    note_dev: "Dev mode is on", "Включён режим разработки";
    note_dev_local: "Dev mode is on · local map", "Включён режим разработки · локальная карта";
    dev_map: "Game map", "Карта игры";
    dev_installer: "Plugin installer", "Установщик плагинов";
    dev_diagnostics: "Diagnostics", "Диагностика";
    dev_plugins: "Plugins", "Плагины";
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

    /// The most a plugin's cache holds, as the approval shows it.
    #[must_use]
    pub fn up_to_gib(self, gib: u32) -> String {
        match self {
            Self::English => format!("up to {gib} GB"),
            Self::Russian => format!("до {gib} ГБ"),
        }
    }

    /// What the player is told a permission allows.
    #[must_use]
    pub fn permission(self, permission: Permission) -> &'static str {
        match self {
            Self::English => permission.description(),
            Self::Russian => match permission {
                Permission::FilesReplace => {
                    "Заменяет файлы игры: шрифты, текстуры, материалы и модели"
                }
                Permission::ChatCommands => "Добавляет свои команды чата",
                Permission::ChatPrint => "Пишет свои сообщения в ваш чат",
                Permission::HttpGet => "Скачивает с этих сайтов",
                Permission::HttpSend => "Отправляет данные на эти сайты",
                Permission::DrawWorld => "Рисует метки, линии и надписи поверх мира игры",
                Permission::MessagesSend => "Отправляет сообщения этим плагинам",
                Permission::ClipboardWrite => "Копирует текст в буфер обмена, когда вы просите",
                Permission::Notifications => {
                    "Показывает уведомления и мигает игрой на панели задач"
                }
                Permission::SoundPlay => "Проигрывает звуки",
                Permission::StorageCache => "Хранит кеш файлов на вашем диске",
                Permission::HttpWebsocket => "Держит постоянные соединения с этими сайтами",
            },
        }
    }

    /// What the player is told a plugin may do: a host permission's own
    /// words, or a domain's from the game map.
    #[must_use]
    pub fn asked(self, asked: &crate::manifest::Asked) -> String {
        match asked {
            crate::manifest::Asked::Permission(permission) => self.permission(*permission).into(),
            crate::manifest::Asked::Read(domain) => match self {
                Self::English => domain.read.en.clone(),
                Self::Russian => domain.read.ru.clone(),
            },
            crate::manifest::Asked::Act(domain) | crate::manifest::Asked::Change(domain) => {
                let words = match asked {
                    crate::manifest::Asked::Change(_) => domain.change.as_ref(),
                    _ => domain.act.as_ref(),
                }
                .unwrap_or(&domain.read);
                match self {
                    Self::English => words.en.clone(),
                    Self::Russian => words.ru.clone(),
                }
            }
        }
    }

    /// The title of a class of permissions.
    #[must_use]
    pub fn class(self, class: Class) -> &'static str {
        match self {
            Self::English => class.title(),
            Self::Russian => match class {
                Class::Acts => "Действует в игре за вас",
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
    /// The theme the player chose; the light one without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// How large the player wants Angelica's windows; normal without a
    /// choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// The key the player chose for the menu; `Insert` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<MenuKey>,
    /// The key the player chose to stop plugins acting; `End` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_key: Option<MenuKey>,
    /// The update channel; the one the build was released on without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<angelica_update::Channel>,
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
            Permission::FilesReplace,
            Permission::ChatCommands,
            Permission::ChatPrint,
            Permission::HttpGet,
            Permission::HttpSend,
            Permission::DrawWorld,
            Permission::MessagesSend,
            Permission::ClipboardWrite,
            Permission::Notifications,
            Permission::SoundPlay,
            Permission::StorageCache,
            Permission::HttpWebsocket,
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

    fn settings_with_format(settings: &Settings) -> Settings {
        Settings {
            format: 1,
            ..settings.clone()
        }
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
        let settings = Settings {
            theme: Some(Theme::Dark),
            size: Some(Size::Small),
            ..settings
        };
        settings.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"format":1,"language":"ru","theme":"dark","size":"small"}"#
        );
        assert_eq!(Settings::load(&path), settings_with_format(&settings));
        let settings = Settings {
            key: MenuKey::from_virtual_key(0x78),
            ..settings
        };
        settings.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"format":1,"language":"ru","theme":"dark","size":"small","key":"F9"}"#
        );
        assert_eq!(Settings::load(&path), settings_with_format(&settings));
        std::fs::write(&path, r#"{"format":1,"key":"A"}"#).unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::write(&path, r#"{"format":2,"language":"ru"}"#).unwrap();
        assert_eq!(Settings::load(&path).language, None);
    }
}
