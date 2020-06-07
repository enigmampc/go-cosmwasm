mod api;
mod db;
mod error;
mod memory;

pub use api::GoApi;
pub use db::{db_t, DB};
pub use memory::{free_rust, Buffer};

use snafu::ResultExt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::from_utf8;
// use std::Vec;

use crate::error::{clear_error, handle_c_error, set_error};
use crate::error::{empty_err, EmptyArg, Error, Panic, Utf8Err, WasmErr};
use cosmwasm_sgx_vm::instance::untrusted_init_bootstrap;
use cosmwasm_sgx_vm::{call_handle_raw, call_init_raw, call_query_raw, CosmCache, Extern};
use cosmwasm_sgx_vm::{
    create_attestation_report_u, untrusted_get_encrypted_seed, untrusted_init_node,
    untrusted_key_gen,
};
use ctor::ctor;
use log;
use log::*;

#[ctor]
fn init_logger() {
    simple_logger::init().unwrap();
}

#[repr(C)]
pub struct cache_t {}

fn to_cache(ptr: *mut cache_t) -> Option<&'static mut CosmCache<DB, GoApi>> {
    if ptr.is_null() {
        None
    } else {
        let c = unsafe { &mut *(ptr as *mut CosmCache<DB, GoApi>) };
        Some(c)
    }
}

#[no_mangle]
pub extern "C" fn get_encrypted_seed(cert: Buffer, err: Option<&mut Buffer>) -> Buffer {
    info!("Hello from get_encrypted_seed");
    let cert_slice = match cert.read() {
        None => {
            set_error("Attestation Certificate is empty".to_string(), err);
            return Buffer::default();
        }
        Some(r) => r,
    };
    info!("Hello from right before untrusted_get_encrypted_seed");
    let result = match untrusted_get_encrypted_seed(cert_slice) {
        Err(e) => {
            error!("Error :(");
            set_error(e.to_string(), err);
            return Buffer::default();
        }
        Ok(r) => {
            clear_error();
            Buffer::from_vec(r.to_vec())
        }
    };
    return result;
}

#[no_mangle]
pub extern "C" fn init_bootstrap(err: Option<&mut Buffer>) -> Buffer {
    info!("Hello from right before init_bootstrap");
    match untrusted_init_bootstrap() {
        Err(e) => {
            error!("Error :(");
            set_error(e.to_string(), err);
            Buffer::default()
        }
        Ok(r) => {
            clear_error();
            Buffer::from_vec(r.to_vec())
        }
    }
}

#[no_mangle]
pub extern "C" fn init_node(
    master_cert: Buffer,
    encrypted_seed: Buffer,
    err: Option<&mut Buffer>,
) -> bool {
    let pk_slice = match master_cert.read() {
        None => {
            set_error("Public key is empty".to_string(), err);
            return false;
        }
        Some(r) => r,
    };
    let encrypted_seed_slice = match encrypted_seed.read() {
        None => {
            set_error("Encrypted seed is empty".to_string(), err);
            return false;
        }
        Some(r) => r,
    };

    let result = match untrusted_init_node(
        pk_slice.as_ptr(),
        pk_slice.len() as u32,
        encrypted_seed_slice.as_ptr(),
        encrypted_seed_slice.len() as u32,
    ) {
        Ok(_) => {
            clear_error();
            true
        }
        Err(e) => {
            set_error(e.to_string(), err);
            false
        }
    };

    result
}

#[no_mangle]
pub extern "C" fn create_attestation_report(err: Option<&mut Buffer>) -> bool {
    if let Err(status) = create_attestation_report_u() {
        set_error(status.to_string(), err);
        return false;
    }
    clear_error();
    true
}

fn to_extern(storage: DB, api: GoApi) -> Extern<DB, GoApi> {
    Extern { storage, api }
}

#[no_mangle]
pub extern "C" fn init_cache(
    data_dir: Buffer,
    cache_size: usize,
    err: Option<&mut Buffer>,
) -> *mut cache_t {
    let r =
        catch_unwind(|| do_init_cache(data_dir, cache_size)).unwrap_or_else(|_| Panic {}.fail());
    match r {
        Ok(t) => {
            clear_error();
            t as *mut cache_t
        }
        Err(e) => {
            set_error(e.to_string(), err);
            std::ptr::null_mut()
        }
    }
}

// store some common string for argument names
static DATA_DIR_ARG: &str = "data_dir";
static CACHE_ARG: &str = "cache";
static WASM_ARG: &str = "wasm";
static CODE_ID_ARG: &str = "code_id";
static MSG_ARG: &str = "msg";
static PARAMS_ARG: &str = "params";
static GAS_USED_ARG: &str = "gas_used";

fn do_init_cache(data_dir: Buffer, cache_size: usize) -> Result<*mut CosmCache<DB, GoApi>, Error> {
    let dir = data_dir.read().ok_or_else(|| empty_err(DATA_DIR_ARG))?;
    let dir_str = from_utf8(dir).context(Utf8Err {})?;
    let cache = unsafe { CosmCache::new(dir_str, cache_size).context(WasmErr {})? };
    let out = Box::new(cache);
    Ok(Box::into_raw(out))
}

/// frees a cache reference
///
/// # Safety
///
/// This must be called exactly once for any `*cache_t` returned by `init_cache`
/// and cannot be called on any other pointer.
#[no_mangle]
pub unsafe extern "C" fn release_cache(cache: *mut cache_t) {
    if !cache.is_null() {
        // this will free cache when it goes out of scope
        let _ = Box::from_raw(cache as *mut CosmCache<DB, GoApi>);
    }
}

#[no_mangle]
pub extern "C" fn create(cache: *mut cache_t, wasm: Buffer, err: Option<&mut Buffer>) -> Buffer {
    let r = match to_cache(cache) {
        Some(c) => catch_unwind(AssertUnwindSafe(move || do_create(c, wasm)))
            .unwrap_or_else(|_| Panic {}.fail()),
        None => EmptyArg { name: CACHE_ARG }.fail(),
    };
    let v = handle_c_error(r, err);
    Buffer::from_vec(v)
}

fn do_create(cache: &mut CosmCache<DB, GoApi>, wasm: Buffer) -> Result<Vec<u8>, Error> {
    let wasm = wasm.read().ok_or_else(|| empty_err(WASM_ARG))?;
    cache.save_wasm(wasm).context(WasmErr {})
}

#[no_mangle]
pub extern "C" fn get_code(cache: *mut cache_t, id: Buffer, err: Option<&mut Buffer>) -> Buffer {
    let r = match to_cache(cache) {
        Some(c) => catch_unwind(AssertUnwindSafe(move || do_get_code(c, id)))
            .unwrap_or_else(|_| Panic {}.fail()),
        None => EmptyArg { name: CACHE_ARG }.fail(),
    };
    let v = handle_c_error(r, err);
    Buffer::from_vec(v)
}

fn do_get_code(cache: &mut CosmCache<DB, GoApi>, id: Buffer) -> Result<Vec<u8>, Error> {
    let id = id.read().ok_or_else(|| empty_err(CACHE_ARG))?;
    cache.load_wasm(id).context(WasmErr {})
}

#[no_mangle]
pub extern "C" fn instantiate(
    cache: *mut cache_t,
    contract_id: Buffer,
    params: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
    err: Option<&mut Buffer>,
) -> Buffer {
    let r = match to_cache(cache) {
        Some(c) => catch_unwind(AssertUnwindSafe(move || {
            do_init(c, contract_id, params, msg, db, api, gas_limit, gas_used)
        }))
        .unwrap_or_else(|_| Panic {}.fail()),
        None => EmptyArg { name: CACHE_ARG }.fail(),
    };
    let v = handle_c_error(r, err);
    Buffer::from_vec(v)
}

fn do_init(
    cache: &mut CosmCache<DB, GoApi>,
    code_id: Buffer,
    params: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
) -> Result<Vec<u8>, Error> {
    let gas_used = gas_used.ok_or_else(|| empty_err(GAS_USED_ARG))?;
    let code_id = code_id.read().ok_or_else(|| empty_err(CODE_ID_ARG))?;
    let params = params.read().ok_or_else(|| empty_err(PARAMS_ARG))?;
    let msg = msg.read().ok_or_else(|| empty_err(MSG_ARG))?;

    let deps = to_extern(db, api);
    let mut instance = cache
        .get_instance(code_id, deps, gas_limit)
        .context(WasmErr {})?;
    let res = call_init_raw(&mut instance, params, msg).context(WasmErr {})?;
    *gas_used = gas_limit - instance.get_gas();
    cache.store_instance(code_id, instance);
    Ok(res)
}

#[no_mangle]
pub extern "C" fn handle(
    cache: *mut cache_t,
    code_id: Buffer,
    params: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
    err: Option<&mut Buffer>,
) -> Buffer {
    let r = match to_cache(cache) {
        Some(c) => catch_unwind(AssertUnwindSafe(move || {
            do_handle(c, code_id, params, msg, db, api, gas_limit, gas_used)
        }))
        .unwrap_or_else(|_| Panic {}.fail()),
        None => EmptyArg { name: CACHE_ARG }.fail(),
    };
    let v = handle_c_error(r, err);
    Buffer::from_vec(v)
}

fn do_handle(
    cache: &mut CosmCache<DB, GoApi>,
    code_id: Buffer,
    params: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
) -> Result<Vec<u8>, Error> {
    let gas_used = gas_used.ok_or_else(|| empty_err(GAS_USED_ARG))?;
    let code_id = code_id.read().ok_or_else(|| empty_err(CODE_ID_ARG))?;
    let params = params.read().ok_or_else(|| empty_err(PARAMS_ARG))?;
    let msg = msg.read().ok_or_else(|| empty_err(MSG_ARG))?;

    let deps = to_extern(db, api);
    let mut instance = cache
        .get_instance(code_id, deps, gas_limit)
        .context(WasmErr {})?;
    let res = call_handle_raw(&mut instance, params, msg).context(WasmErr {})?;
    *gas_used = gas_limit - instance.get_gas();
    cache.store_instance(code_id, instance);
    Ok(res)
}

#[no_mangle]
pub extern "C" fn query(
    cache: *mut cache_t,
    code_id: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
    err: Option<&mut Buffer>,
) -> Buffer {
    let r = match to_cache(cache) {
        Some(c) => catch_unwind(AssertUnwindSafe(move || {
            do_query(c, code_id, msg, db, api, gas_limit, gas_used)
        }))
        .unwrap_or_else(|_| Panic {}.fail()),
        None => EmptyArg { name: CACHE_ARG }.fail(),
    };
    let v = handle_c_error(r, err);
    Buffer::from_vec(v)
}

fn do_query(
    cache: &mut CosmCache<DB, GoApi>,
    code_id: Buffer,
    msg: Buffer,
    db: DB,
    api: GoApi,
    gas_limit: u64,
    gas_used: Option<&mut u64>,
) -> Result<Vec<u8>, Error> {
    let gas_used = gas_used.ok_or_else(|| empty_err(GAS_USED_ARG))?;
    let code_id = code_id.read().ok_or_else(|| empty_err(CODE_ID_ARG))?;
    let msg = msg.read().ok_or_else(|| empty_err(MSG_ARG))?;

    let deps = to_extern(db, api);
    let mut instance = cache
        .get_instance(code_id, deps, gas_limit)
        .context(WasmErr {})?;
    let res = call_query_raw(&mut instance, msg).context(WasmErr {})?;
    *gas_used = gas_limit - instance.get_gas();
    cache.store_instance(code_id, instance);
    Ok(res)
}

#[no_mangle]
pub extern "C" fn key_gen(err: Option<&mut Buffer>) -> Buffer {
    info!("Hello from right before key_gen");
    match untrusted_key_gen() {
        Err(e) => {
            error!("Error :(");
            set_error(e.to_string(), err);
            Buffer::default()
        }
        Ok(r) => {
            clear_error();
            Buffer::from_vec(r.to_vec())
        }
    }
}
