//! The WASI imports a plugin gets: random numbers and clocks. They expose
//! nothing of the player's system, and the Rust standard library needs them
//! (`HashMap` seeds its hasher from `wasi:random`, `Instant` reads
//! `wasi:clocks`). Every other WASI import traps when called.
#![forbid(unsafe_code)]
use crate::sandbox::State;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use wasmtime::component::{ComponentType, Lift, Linker, Lower};

/// The most random bytes one call returns.
const MAX_RANDOM: u64 = 1024 * 1024;

#[derive(ComponentType, Lift, Lower, Clone, Copy)]
#[component(record)]
struct Datetime {
    seconds: u64,
    nanoseconds: u32,
}

fn random(length: u64) -> wasmtime::Result<Vec<u8>> {
    if length > MAX_RANDOM {
        wasmtime::bail!("at most {MAX_RANDOM} random bytes per call");
    }
    let mut bytes = vec![0; usize::try_from(length)?];
    getrandom::fill(&mut bytes).map_err(|e| wasmtime::format_err!("{e}"))?;
    Ok(bytes)
}

fn random_u64() -> wasmtime::Result<u64> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|e| wasmtime::format_err!("{e}"))?;
    Ok(u64::from_le_bytes(bytes))
}

/// Defines the random and clock imports `component` asks for, at the
/// version it asks for.
///
/// # Errors
/// When an import of these interfaces has an unexpected type.
pub fn define(linker: &mut Linker<State>, names: &[String]) -> wasmtime::Result<()> {
    let started = Instant::now();
    for name in names {
        let Some((interface, _version)) = name.split_once('@') else {
            continue;
        };
        match interface {
            "wasi:random/random" => {
                let mut instance = linker.instance(name)?;
                instance.func_wrap("get-random-bytes", |_, (length,): (u64,)| {
                    Ok((random(length)?,))
                })?;
                instance.func_wrap("get-random-u64", |_, (): ()| Ok((random_u64()?,)))?;
            }
            "wasi:random/insecure" => {
                let mut instance = linker.instance(name)?;
                instance.func_wrap("get-insecure-random-bytes", |_, (length,): (u64,)| {
                    Ok((random(length)?,))
                })?;
                instance.func_wrap("get-insecure-random-u64", |_, (): ()| Ok((random_u64()?,)))?;
            }
            "wasi:random/insecure-seed" => {
                linker
                    .instance(name)?
                    .func_wrap("insecure-seed", |_, (): ()| {
                        Ok(((random_u64()?, random_u64()?),))
                    })?;
            }
            "wasi:clocks/monotonic-clock" => {
                let mut instance = linker.instance(name)?;
                instance.func_wrap("now", move |_, (): ()| {
                    Ok((u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),))
                })?;
                instance.func_wrap("resolution", |_, (): ()| Ok((1u64,)))?;
            }
            "wasi:clocks/wall-clock" => {
                let mut instance = linker.instance(name)?;
                instance.func_wrap("now", |_, (): ()| {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default();
                    Ok((Datetime {
                        seconds: now.as_secs(),
                        nanoseconds: now.subsec_nanos(),
                    },))
                })?;
                instance.func_wrap("resolution", |_, (): ()| {
                    Ok((Datetime {
                        seconds: 0,
                        nanoseconds: 1,
                    },))
                })?;
            }
            _ => {}
        }
    }
    Ok(())
}
