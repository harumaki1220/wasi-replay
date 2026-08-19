use anyhow::Result;
use rand::TryRng;
use std::convert::Infallible;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

struct FixedRandom;

impl TryRng for FixedRandom {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(42)
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(42)
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        dest.fill(42);
        Ok(())
    }
}

struct HostState {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

fn main() -> Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(&engine, "guest/target/wasm32-wasip1/release/guest.wasm")?;

    let mut linker = Linker::<HostState>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    let state = HostState {
        ctx: WasiCtxBuilder::new()
            .inherit_stdio()
            .secure_random(FixedRandom)
            .build(),
        table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);

    let instance = linker.instantiate(&mut store, &component)?;

    let func = instance.get_typed_func::<(), (String,)>(&mut store, "hello-world")?;
    let (result,) = func.call(&mut store, ())?;

    println!("{}", result);
    Ok(())
}
