use std::{ffi::c_void, sync::OnceLock};
use windows::{
    Win32::Graphics::Direct3D::{D3D_SHADER_MACRO, ID3DBlob},
    core::{HRESULT, PCSTR},
};

type Compile = unsafe extern "system" fn(
    *const c_void,
    usize,
    PCSTR,
    *const D3D_SHADER_MACRO,
    *mut c_void,
    PCSTR,
    PCSTR,
    u32,
    u32,
    *mut Option<ID3DBlob>,
    *mut Option<ID3DBlob>,
) -> HRESULT;

fn compiler() -> Result<Compile, String> {
    static COMPILER: OnceLock<Result<Compile, String>> = OnceLock::new();
    COMPILER
        .get_or_init(|| {
            let address = super::system_dll::export("d3dcompiler_47.dll", c"D3DCompile")?;
            // SAFETY: This export has the declared Windows ABI. Its module stays loaded.
            Ok(unsafe { std::mem::transmute::<super::system_dll::Export, Compile>(address) })
        })
        .clone()
}

pub(super) fn check() -> Result<(), String> {
    compiler().map(|_| ())
}

pub(super) fn compile(
    source: &str,
    name: PCSTR,
    entry: PCSTR,
    target: PCSTR,
) -> Result<Vec<u8>, String> {
    let compile = compiler()?;
    let mut code = None;
    let mut error = None;
    // SAFETY: Source and names live for the call. No defines or include handler are provided.
    let result = unsafe {
        compile(
            source.as_ptr().cast(),
            source.len(),
            name,
            std::ptr::null(),
            std::ptr::null_mut(),
            entry,
            target,
            0,
            0,
            &raw mut code,
            &raw mut error,
        )
        .ok()
    };
    let bytes = |blob: &ID3DBlob| {
        // SAFETY: A blob owns readable bytes for exactly its size.
        unsafe {
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
        }
        .to_vec()
    };
    match (result, code) {
        (Ok(()), Some(code)) => Ok(bytes(&code)),
        (result, _) => Err(error.as_ref().map_or_else(
            || {
                result
                    .err()
                    .map_or_else(|| "no shader bytecode".into(), |e| e.to_string())
            },
            |error| {
                String::from_utf8_lossy(&bytes(error))
                    .trim_end_matches('\0')
                    .trim()
                    .to_owned()
            },
        )),
    }
}
