extern crate holochain_cas_implementations;
extern crate holochain_container_api;
extern crate holochain_core;
extern crate holochain_core_types;
extern crate holochain_net;
extern crate tempfile;
extern crate wabt;
#[macro_use]
extern crate serde_json;


use holochain_core_types::agent::Agent;
use holochain_cas_implementations::{cas::file::FilesystemStorage, eav::file::EavFileStorage};
use holochain_container_api::{error::HolochainResult, Holochain};
use holochain_core::{context::Context, logger::Logger, persister::SimplePersister};
use holochain_core_types::json::JsonString;
use holochain_core_types::dna::{
    wasm::DnaWasm,
    zome::{
        capabilities::{Capability, FnDeclaration, Membrane},
        entry_types::{EntryTypeDef, LinksTo, LinkedFrom},
        Config, Zome,
    },
    Dna,
};
use holochain_net::p2p_network::P2pNetwork;

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    fmt,
    fs::File,
    hash::{Hash, Hasher},
    io::prelude::*,
    sync::{Arc, Mutex,RwLock},
};
use tempfile::tempdir;
use wabt::Wat2Wasm;

/// Load WASM from filesystem
pub fn create_wasm_from_file(fname: &str) -> Vec<u8> {
    let mut file = File::open(fname).unwrap();
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).unwrap();
    buf
}

/// Create DNA from WAT
pub fn create_test_dna_with_wat(zome_name: &str, cap_name: &str, wat: Option<&str>) -> Dna {
    // Default WASM code returns 1337 as integer
    let default_wat = r#"
            (module
                (memory (;0;) 17)
                (func (export "main") (param $p0 i32) (result i32)
                    i32.const 6
                )
                (data (i32.const 0)
                    "1337.0"
                )
                (export "memory" (memory 0))
            )
        "#;
    let wat_str = wat.unwrap_or_else(|| &default_wat);

    // Test WASM code that returns 1337 as integer
    let wasm_binary = Wat2Wasm::new()
        .canonicalize_lebs(false)
        .write_debug_names(true)
        .convert(wat_str)
        .unwrap();

    create_test_dna_with_wasm(zome_name, cap_name, wasm_binary.as_ref().to_vec())
}

/// Prepare valid DNA struct with that WASM in a zome's capability
pub fn create_test_dna_with_wasm(zome_name: &str, cap_name: &str, wasm: Vec<u8>) -> Dna {
    let mut dna = Dna::new();
    let capability = create_test_cap_with_fn_name("main");

    let mut capabilities = HashMap::new();
    capabilities.insert(cap_name.to_string(), capability);

    let mut test_entry_def = EntryTypeDef::new();
    test_entry_def.links_to.push(LinksTo {
        target_type: String::from("testEntryType"),
        tag: String::from("test-tag"),
    });

    let mut test_entry_b_def = EntryTypeDef::new();
    test_entry_b_def.linked_from.push(LinkedFrom {
        base_type: String::from("testEntryType"),
        tag: String::from("test-tag"),
    });

    let mut entry_types = HashMap::new();
    entry_types.insert(String::from("testEntryType"), test_entry_def);
    entry_types.insert(String::from("testEntryTypeB"), test_entry_b_def);

    let zome = Zome::new(
        "some zome description",
        &Config::new(),
        &entry_types,
        &capabilities,
        &DnaWasm { code: wasm },
    );

    // zome.capabilities.push(capability);
    dna.zomes.insert(zome_name.to_string(), zome);
    dna.name = "TestApp".into();
    dna.uuid = "8ed84a02-a0e6-4c8c-a752-34828e302986".into();
    dna
}

pub fn create_test_cap(membrane: Membrane) -> Capability {
    let mut capability = Capability::new();
    capability.cap_type.membrane = membrane;
    capability
}

pub fn create_test_cap_with_fn_name(fn_name: &str) -> Capability {
    let mut capability = Capability::new();
    let mut fn_decl = FnDeclaration::new();
    fn_decl.name = String::from(fn_name);
    capability.functions.push(fn_decl);
    capability
}

/// Prepare valid DNA struct with that WASM in a zome's capability
pub fn create_test_dna_with_cap(
    zome_name: &str,
    cap_name: &str,
    cap: &Capability,
    wasm: &[u8],
) -> Dna {
    let mut dna = Dna::new();

    let mut capabilities = HashMap::new();
    capabilities.insert(cap_name.to_string(), cap.clone());

    let etypedef = EntryTypeDef::new();
    let mut entry_types = HashMap::new();
    entry_types.insert("testEntryType".to_string(), etypedef);
    let zome = Zome::new(
        "some zome description",
        &Config::new(),
        &entry_types,
        &capabilities,
        &DnaWasm {
            code: wasm.to_owned(),
        },
    );

    dna.zomes.insert(zome_name.to_string(), zome);
    dna.name = "TestApp".into();
    dna.uuid = "8ed84a02-a0e6-4c8c-a752-34828e302986".into();
    dna
}

#[derive(Clone)]
pub struct TestLogger {
    pub log: Vec<String>,
}

impl Logger for TestLogger {
    fn log(&mut self, msg: String) {
        self.log.push(msg);
    }
    fn dump(&self) -> String {
        format!("{:?}", self.log)
    }
}

// trying to get a way to print out what has been logged for tests without a read function.
// this currently fails
impl fmt::Debug for TestLogger {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.log)
    }
}

pub fn test_logger() -> Arc<Mutex<TestLogger>> {
    Arc::new(Mutex::new(TestLogger { log: Vec::new() }))
}

/// create a test network
#[cfg_attr(tarpaulin, skip)]
fn make_mock_net() -> Arc<Mutex<P2pNetwork>> {
    let res = P2pNetwork::new(
        Box::new(|_r| Ok(())),
        &json!({
            "backend": "mock"
        }).into(),
    ).unwrap();
    Arc::new(Mutex::new(res))
}

#[cfg_attr(tarpaulin, skip)]
pub fn test_context_and_logger(agent_name: &str) -> (Arc<Context>, Arc<Mutex<TestLogger>>) {
    let agent = Agent::generate_fake(agent_name);
    let file_storage = Arc::new(RwLock::new(FilesystemStorage::new(tempdir().unwrap().path().to_str().unwrap()).unwrap()));
    let logger = test_logger();
    (
        Arc::new(
            Context::new(
                agent,
                logger.clone(),
                Arc::new(Mutex::new(SimplePersister::new(file_storage.clone()))),
                file_storage.clone(),
                Arc::new(RwLock::new(EavFileStorage::new(tempdir().unwrap().path().to_str().unwrap().to_string()).unwrap())),
                make_mock_net(),
            ).unwrap(),
        ),
        logger,
    )
}

pub fn test_context(agent_name: &str) -> Arc<Context> {
    let (context, _) = test_context_and_logger(agent_name);
    context
}

/// calculates the native Rust hash
/// has nothing to do with our hashing e.g. multihash
/// @see https://doc.rust-lang.org/std/hash/index.html
pub fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

// Function called at start of all unit tests:
//   Startup holochain and do a call on the specified wasm function.
pub fn hc_setup_and_call_zome_fn(wasm_path: &str, fn_name: &str) -> HolochainResult<JsonString> {
    // Setup the holochain instance
    let wasm = create_wasm_from_file(wasm_path);
    let capability = create_test_cap_with_fn_name(fn_name);
    let dna = create_test_dna_with_cap("test_zome", "test_cap", &capability, &wasm);

    let context = create_test_context("alex");
    let mut hc = Holochain::new(dna.clone(), context).unwrap();

    // Run the holochain instance
    hc.start().expect("couldn't start");
    // Call the exposed wasm function
    return hc.call("test_zome", "test_cap", fn_name, r#"{}"#);
}

/// create a test context and TestLogger pair so we can use the logger in assertions
pub fn create_test_context(agent_name: &str) -> Arc<Context> {
    let agent = Agent::generate_fake(agent_name);
    let logger = test_logger();

    let file_storage = Arc::new(RwLock::new(FilesystemStorage::new(tempdir().unwrap().path().to_str().unwrap()).unwrap()));
    Arc::new(
        Context::new(
            agent,
            logger.clone(),
            Arc::new(Mutex::new(SimplePersister::new(file_storage.clone()))),
            file_storage.clone(),
            Arc::new(RwLock::new(EavFileStorage::new(tempdir().unwrap().path().to_str().unwrap().to_string()).unwrap())),
            make_mock_net(),
        ).unwrap(),
    )
}
