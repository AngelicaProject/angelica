//! A test plugin that intercepts the scripted game's functions in every way
//! the host allows. The host's tests click `before:<function>:<handler>` or
//! `after:<function>:<handler>` to intercept, and each handler number does
//! one thing to the call; the log tells what happened.
use angelica_sdk::{
    Plugin,
    intercepts::{self, Call, Phase, Value},
    ui::{Event, Window},
};

struct Intercepting;

fn unsigned(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Unsigned(number)) => *number,
        _ => 0,
    }
}

fn refused<T>(result: Result<T, angelica_sdk::Error>) -> String {
    match result {
        Ok(_) => "accepted".into(),
        Err(error) => format!("{:?}", error.kind),
    }
}

impl Plugin for Intercepting {
    fn start() -> Result<Self, String> {
        Ok(Self)
    }

    fn ui(&mut self) -> Vec<Window> {
        vec![Window::new("main", "Intercepting")]
    }

    fn ui_event(&mut self, event: Event) {
        let Event::Clicked(target) = event else {
            return;
        };
        let mut parts = target.id.split(':');
        let (Some(phase), Some(function), Some(Ok(handler))) =
            (parts.next(), parts.next(), parts.next().map(str::parse))
        else {
            return;
        };
        let result = match phase {
            "before" => intercepts::before(function, handler),
            "after" => intercepts::after(function, handler),
            _ => return,
        };
        angelica_sdk::info(&format!(
            "{phase} {function} {handler}: {}",
            refused(result)
        ));
    }

    /// 0 logs the call; 1 doubles the first argument and 2 adds one to it;
    /// 3 skips the game's code, returning 99; 4 doubles the result and 5
    /// adds one to it; 6 tries what a `before` handler may not; 7 changes
    /// the first argument and traps; 8 doubles the float arguments and adds
    /// one to the fifth, on the stack; 9 passes the second argument's
    /// handle on as it is, then a zero pointer.
    fn intercepted(call: Call) {
        let first = unsigned(call.args.first());
        let returned = unsigned(Some(&call.returned));
        let outcome = match call.handler {
            0 => {
                angelica_sdk::info(&format!(
                    "{:?} args={:?} returned={:?} skipped={}",
                    call.phase, call.args, call.returned, call.skipped
                ));
                Ok(())
            }
            1 => intercepts::set_argument(0, &Value::Unsigned(first * 2)),
            2 => intercepts::set_argument(0, &Value::Unsigned(first + 1)),
            3 => intercepts::skip(&Value::Unsigned(99)),
            4 => intercepts::set_result(&Value::Unsigned(returned * 2)),
            5 => intercepts::set_result(&Value::Unsigned(returned + 1)),
            6 => {
                let attempts = [
                    refused(intercepts::set_argument(0, &Value::Signed(1))),
                    refused(intercepts::set_argument(0, &Value::Unsigned(1 << 40))),
                    refused(intercepts::set_argument(9, &Value::Unsigned(1))),
                    refused(intercepts::set_result(&Value::Unsigned(1))),
                    refused(intercepts::skip(&Value::Boolean(true))),
                    refused(intercepts::set_argument(1, &Value::Pointer(1))),
                ];
                angelica_sdk::info(&format!("attempts: {}", attempts.join(", ")));
                Ok(())
            }
            7 => {
                let _ = intercepts::set_argument(0, &Value::Unsigned(1000));
                panic!("the handler fails after changing an argument");
            }
            8 => {
                let mut outcome = Ok(());
                for (index, value) in call.args.iter().enumerate() {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    let changed = match value {
                        Value::Float32(number) => Value::Float32(number * 2.0),
                        Value::Float64(number) => Value::Float64(number * 2.0),
                        Value::Unsigned(number) if index == 4 => Value::Unsigned(number + 1),
                        _ => continue,
                    };
                    outcome = outcome.and(intercepts::set_argument(index, &changed));
                }
                outcome
            }
            9 => match call.args.get(1) {
                Some(text @ Value::Text(handle)) => {
                    let read = intercepts::text(*handle)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                    angelica_sdk::info(&format!("text: {read:?}"));
                    intercepts::set_argument(1, text)
                        .and_then(|()| intercepts::set_argument(1, &Value::None))
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            angelica_sdk::warning(&format!(
                "handler {} in {:?}: {:?} {}",
                call.handler, call.phase, error.kind, error.message
            ));
        }
        if call.phase == Phase::After && call.skipped {
            angelica_sdk::info(&format!("after a skip: {:?}", call.returned));
        }
    }
}

angelica_sdk::plugin!(Intercepting);
