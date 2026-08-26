use anyhow::Result;
use std::convert::Infallible;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use rand::{Rng, SeedableRng, TryRng};
use std::sync::{Arc, Mutex};

/// 記録・再生する乱数装置
enum ReplayRandom {
    Record {
        inner: rand::rngs::StdRng,
        log: Arc<Mutex<Vec<u64>>>,
    },
    Replay {
        log: Vec<u64>,
        pos: Arc<Mutex<usize>>,
    },
}

impl TryRng for ReplayRandom {
    type Error = Infallible;

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        match self {
            ReplayRandom::Record { inner, log } => {
                let v = inner.next_u64();
                log.lock().unwrap().push(v);
                Ok(v)
            }
            ReplayRandom::Replay { log, pos } => {
                let mut i = pos.lock().unwrap();
                let v = *log.get(*i).unwrap_or_else(|| {
                    panic!(
                        "記録より多くの乱数が要求されました（記録: {} 件、{} 回目の要求）。\
                         記録時と再生時でコードが変わっていませんか？",
                        log.len(),
                        *i + 1
                    )
                });
                *i += 1;
                Ok(v)
            }
        }
    }

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.try_next_u64()? as u32)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        for chunk in dest.chunks_mut(8) {
            let v = self.try_next_u64()?.to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
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
    // 引数を1つ読む。record か replay か。
    let mode = std::env::args().nth(1).unwrap_or_default();
    let log_path = "random.log";

    // 記録モードのときだけ使う共有の箱
    let recorded = Arc::new(Mutex::new(Vec::<u64>::new()));

    let consumed = Arc::new(Mutex::new(0usize));
    let mut total = 0usize;

    let rng = match mode.as_str() {
        "record" => ReplayRandom::Record {
            inner: rand::rngs::StdRng::from_rng(&mut rand::rng()),
            log: Arc::clone(&recorded),
        },
        "replay" => {
            let text = std::fs::read_to_string(log_path)?;
            let log = text
                .lines()
                .map(|line| line.trim().parse::<u64>())
                .collect::<std::result::Result<Vec<u64>, _>>()?;
            total = log.len();
            ReplayRandom::Replay {
                log,
                pos: Arc::clone(&consumed),
            }
        }
        other => anyhow::bail!(
            "使い方: cargo run -p host -- <record|replay> (受け取った引数: {other:?})"
        ),
    };

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(&engine, "guest/target/wasm32-wasip1/release/guest.wasm")?;

    let mut linker = Linker::<HostState>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    let state = HostState {
        ctx: WasiCtxBuilder::new()
            .inherit_stdio()
            .secure_random(rng)
            .build(),
        table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);

    let instance = linker.instantiate(&mut store, &component)?;
    let func = instance.get_typed_func::<(), (String,)>(&mut store, "hello-world")?;
    let (result,) = func.call(&mut store, ())?;

    println!("{}", result);

    // 記録モードなら、溜まった値をファイルに書き出す
    if mode == "record" {
        let values = recorded.lock().unwrap();
        let text = values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(log_path, text)?;
        println!("記録しました: {} 件 -> {}", values.len(), log_path);
    }

    if mode == "replay" {
        let used = *consumed.lock().unwrap();
        if used < total {
            eprintln!(
                "警告: ログを使い切っていません（{used} / {total} 件）。\
                 記録時とコードが違う可能性があります。"
            );
        }
    }

    Ok(())
}
