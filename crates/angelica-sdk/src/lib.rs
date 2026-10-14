//! Write Angelica plugins in Rust.
//!
//! A plugin implements [`Plugin`] and exports it with [`plugin!`]. It is
//! built for `wasm32-wasip2`. The host calls the plugin's main instance on
//! the game's main thread; [`Plugin::run_job`] runs in a separate instance
//! (see [`api::jobs`]).
//!
//! ```ignore
//! use angelica_sdk::ui;
//!
//! struct Hello {
//!     clicks: u32,
//! }
//!
//! impl angelica_sdk::Plugin for Hello {
//!     fn start() -> Result<Self, String> {
//!         Ok(Self { clicks: 0 })
//!     }
//!
//!     fn ui(&mut self) -> Vec<ui::Window> {
//!         vec![ui::Window::new("main", "Hello").text(format!("{} clicks", self.clicks)).button("click", "Click")]
//!     }
//!
//!     fn ui_event(&mut self, event: ui::Event) {
//!         if event.clicked("main", "click") {
//!             self.clicks += 1;
//!         }
//!     }
//! }
//!
//! angelica_sdk::plugin!(Hello);
//! ```

#[doc(hidden)]
pub mod bindings {
    // Canonical ABI allocations carry a length and allocate exactly that capacity.
    #![allow(clippy::same_length_and_capacity)]
    wit_bindgen::generate!({ path: "../../wit", world: "plugin", pub_export_macro: true, generate_all });
}

pub mod chat;
pub mod commands;
pub mod draw;
pub mod events;
pub mod excel;
pub mod game_ui;
pub mod http;
pub mod jobs;
pub mod lens;
pub mod storage;
pub mod ui;

/// The host's interfaces, as generated from `wit/`.
pub mod api {
    pub use crate::bindings::angelica::plugin::{
        chat, commands, draw, events, excel, excel_text, file_replacements, game_files, game_ui,
        http, jobs, lens, runtime, storage, text_case, types,
    };
}

pub use angelica_text as text;
pub use api::types::{Error, ErrorKind};

/// Lets `?` turn a host error into the message that [`Plugin`]'s methods
/// return.
impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.message
    }
}

/// A plugin. The host creates it with [`Plugin::start`] when the plugin is
/// enabled and drops it after [`Plugin::stop`].
pub trait Plugin: Sized + 'static {
    /// # Errors
    /// An error disables the plugin and is shown to the player.
    fn start() -> Result<Self, String>;

    /// Called every frame; `elapsed` is the time in seconds since the
    /// previous `tick`. Sum it to measure time, or use `std::time::Instant`
    /// and `SystemTime`, which work in a plugin.
    ///
    /// # Errors
    /// An error disables the plugin and is shown to the player.
    fn tick(&mut self, _elapsed: f64) -> Result<(), String> {
        Ok(())
    }

    /// The plugin's windows for this frame.
    fn ui(&mut self) -> Vec<ui::Window> {
        Vec::new()
    }

    /// A player action in a window or the Angelica menu.
    fn ui_event(&mut self, _event: ui::Event) {}

    /// A command of the plugin the player ran (see [`commands`]).
    ///
    /// # Errors
    /// The error is printed in the chat log as the plugin's message.
    fn command(&mut self, _invocation: commands::Invocation) -> Result<(), String> {
        Ok(())
    }

    /// An event the plugin subscribed to (see [`events`]).
    fn game_event(&mut self, _event: events::GameEvent) {}

    /// Called before the plugin is disabled.
    fn stop(&mut self) {}

    /// Runs a job in a separate instance, where `start` has not run.
    ///
    /// # Errors
    /// The error is what `jobs.poll` reports.
    fn run_job(name: &str, _input: Vec<u8>) -> Result<Vec<u8>, String> {
        Err(format!("this plugin has no job {name}"))
    }
}

/// Logs a line at a level.
pub fn log(level: api::runtime::Level, message: &str) {
    api::runtime::log(level, message);
}

/// Logs an informational line.
pub fn info(message: &str) {
    log(api::runtime::Level::Info, message);
}

/// Logs a warning.
pub fn warning(message: &str) {
    log(api::runtime::Level::Warning, message);
}

/// Logs an error.
pub fn error(message: &str) {
    log(api::runtime::Level::Error, message);
}

/// Exports a [`Plugin`] as the component's guest.
#[macro_export]
macro_rules! plugin {
    ($plugin:ty) => {
        const _: () = {
            ::std::thread_local! {
                static STATE: ::std::cell::RefCell<::std::option::Option<$plugin>> =
                    const { ::std::cell::RefCell::new(::std::option::Option::None) };
            }

            fn with<R>(default: R, call: impl FnOnce(&mut $plugin) -> R) -> R {
                STATE.with(|state| match state.borrow_mut().as_mut() {
                    ::std::option::Option::Some(plugin) => call(plugin),
                    ::std::option::Option::None => default,
                })
            }

            struct Guest;

            impl $crate::bindings::exports::angelica::plugin::guest::Guest for Guest {
                fn start() -> ::std::result::Result<(), ::std::string::String> {
                    let plugin = <$plugin as $crate::Plugin>::start()?;
                    STATE.with(|state| *state.borrow_mut() = ::std::option::Option::Some(plugin));
                    ::std::result::Result::Ok(())
                }

                fn stop() {
                    with((), $crate::Plugin::stop);
                    STATE.with(|state| *state.borrow_mut() = ::std::option::Option::None);
                }

                fn tick(elapsed: f64) -> ::std::result::Result<(), ::std::string::String> {
                    with(::std::result::Result::Ok(()), |plugin| {
                        $crate::Plugin::tick(plugin, elapsed)
                    })
                }

                fn ui() -> ::std::vec::Vec<$crate::ui::Window> {
                    with(::std::vec::Vec::new(), $crate::Plugin::ui)
                }

                fn ui_event(event: $crate::ui::Event) {
                    with((), |plugin| $crate::Plugin::ui_event(plugin, event));
                }

                fn command(
                    invocation: $crate::commands::Invocation,
                ) -> ::std::result::Result<(), ::std::string::String> {
                    with(::std::result::Result::Ok(()), |plugin| {
                        $crate::Plugin::command(plugin, invocation)
                    })
                }

                fn on_game_event(event: $crate::events::GameEvent) {
                    with((), |plugin| $crate::Plugin::game_event(plugin, event));
                }

                fn run_job(
                    name: ::std::string::String,
                    input: ::std::vec::Vec<u8>,
                ) -> ::std::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                    <$plugin as $crate::Plugin>::run_job(&name, input)
                }
            }

    $crate::bindings::export!(Guest with_types_in $crate::bindings);
        };
    };
}
