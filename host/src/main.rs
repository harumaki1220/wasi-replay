use anyhow::Result;
use rand::{Rng, SeedableRng, TryRng};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{
    HostMonotonicClock, HostWallClock, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
};

// 台帳 (Tape)
// 乱数・実時刻・単調時刻の3つが、この1本の台帳を共有する。
// 呼ばれた順番がそのまま行の順番になるので、
// 再生時に「順序がズレたか」まで検査できる。

#[derive(PartialEq, Debug, Clone, Copy)]
enum Kind {
    Random,
    Wall,
    Monotonic,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Random => "random",
            Kind::Wall => "wall",
            Kind::Monotonic => "monotonic",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "random" => Ok(Kind::Random),
            "wall" => Ok(Kind::Wall),
            "monotonic" => Ok(Kind::Monotonic),
            other => anyhow::bail!("知らない種類です: {other:?}"),
        }
    }
}

enum Tape {
    Record(Vec<(Kind, u64)>),
    Replay {
        entries: Vec<(Kind, u64)>,
        pos: usize,
    },
}

impl Tape {
    /// 記録モード: 値を台帳の末尾に書いて、そのまま返す。
    fn push(&mut self, kind: Kind, value: u64) -> u64 {
        match self {
            Tape::Record(entries) => {
                entries.push((kind, value));
                value
            }
            Tape::Replay { .. } => unreachable!("再生モードで push が呼ばれた"),
        }
    }

    /// 再生モード: 次の1件を取り出す。種類が違えば異なる実行なので止める。
    fn next(&mut self, want: Kind) -> u64 {
        match self {
            Tape::Replay { entries, pos } => {
                let (kind, value) = entries.get(*pos).copied().unwrap_or_else(|| {
                    panic!(
                        "記録より多くの要求がありました（記録: {} 件、{} 回目の要求は {}）。\n\
                         記録時と再生時でコードが変わっていませんか？",
                        entries.len(),
                        *pos + 1,
                        want.as_str()
                    )
                });
                if kind != want {
                    panic!(
                        "{} 回目の要求がズレています（記録: {}、今回: {}）。\n\
                         記録時と再生時で処理の順番が変わっています。",
                        *pos + 1,
                        kind.as_str(),
                        want.as_str()
                    );
                }
                *pos += 1;
                value
            }
            Tape::Record(_) => unreachable!("記録モードで next が呼ばれた"),
        }
    }

    fn is_record(&self) -> bool {
        matches!(self, Tape::Record(_))
    }
}

// 乱数の係

struct ReplayRandom {
    tape: Arc<Mutex<Tape>>,
    inner: Option<rand::rngs::StdRng>, // 記録モードのときだけ中身がある
}

impl TryRng for ReplayRandom {
    type Error = Infallible;

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut tape = self.tape.lock().unwrap();
        let v = match &mut self.inner {
            Some(rng) => {
                let v = rng.next_u64();
                tape.push(Kind::Random, v)
            }
            None => tape.next(Kind::Random),
        };
        Ok(v)
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

// 実時刻の係 (1970年からの経過)
// メソッドが &self なので、自分のフィールドは書き換えられない。
// Mutex の内部可変性で台帳を書き換える。

struct ReplayWallClock {
    tape: Arc<Mutex<Tape>>,
}

impl HostWallClock for ReplayWallClock {
    fn resolution(&self) -> Duration {
        Duration::from_nanos(1)
    }

    fn now(&self) -> Duration {
        let mut tape = self.tape.lock().unwrap();
        let nanos = if tape.is_record() {
            let real = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("1970年より前の時刻")
                .as_nanos() as u64;
            tape.push(Kind::Wall, real)
        } else {
            tape.next(Kind::Wall)
        };
        Duration::from_nanos(nanos)
    }
}

// 単調時刻の係 (起動からの経過。巻き戻らない)

struct ReplayMonotonicClock {
    tape: Arc<Mutex<Tape>>,
    start: std::time::Instant,
}

impl HostMonotonicClock for ReplayMonotonicClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        let mut tape = self.tape.lock().unwrap();
        if tape.is_record() {
            let real = self.start.elapsed().as_nanos() as u64;
            tape.push(Kind::Monotonic, real)
        } else {
            tape.next(Kind::Monotonic)
        }
    }
}

// ホスト側の持ち物

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

// 台帳の読み書き

fn load_tape(path: &str) -> Result<Vec<(Kind, u64)>> {
    let text = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (kind, value) = line
            .split_once(' ')
            .ok_or_else(|| anyhow::anyhow!("{} 行目の形式が不正です: {line:?}", i + 1))?;
        entries.push((Kind::parse(kind)?, value.trim().parse::<u64>()?));
    }
    Ok(entries)
}

fn save_tape(path: &str, entries: &[(Kind, u64)]) -> Result<()> {
    let text = entries
        .iter()
        .map(|(kind, value)| format!("{} {}", kind.as_str(), value))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, text)?;
    Ok(())
}

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let tape_path = "run.tape";

    let tape = match mode.as_str() {
        "record" => Arc::new(Mutex::new(Tape::Record(Vec::new()))),
        "replay" => Arc::new(Mutex::new(Tape::Replay {
            entries: load_tape(tape_path)?,
            pos: 0,
        })),
        other => anyhow::bail!(
            "使い方: cargo run -p host -- <record|replay> (受け取った引数: {other:?})"
        ),
    };

    // 記録モードのときだけ本物の乱数装置を用意する
    let inner = if mode == "record" {
        Some(rand::rngs::StdRng::from_rng(&mut rand::rng()))
    } else {
        None
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
            .secure_random(ReplayRandom {
                tape: Arc::clone(&tape),
                inner,
            })
            .wall_clock(ReplayWallClock {
                tape: Arc::clone(&tape),
            })
            .monotonic_clock(ReplayMonotonicClock {
                tape: Arc::clone(&tape),
                start: std::time::Instant::now(),
            })
            .build(),
        table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);

    let instance = linker.instantiate(&mut store, &component)?;
    let func = instance.get_typed_func::<(), (String,)>(&mut store, "hello-world")?;
    let (result,) = func.call(&mut store, ())?;

    println!("{}", result);

    // 後始末: 記録なら書き出し、再生なら使い切ったか確認
    let tape = tape.lock().unwrap();
    match &*tape {
        Tape::Record(entries) => {
            save_tape(tape_path, entries)?;
            println!("記録しました: {} 件 -> {}", entries.len(), tape_path);
        }
        Tape::Replay { entries, pos } => {
            if *pos < entries.len() {
                eprintln!(
                    "警告: 台帳を使い切っていません（{pos} / {} 件）。\
                     記録時とコードが違う可能性があります。",
                    entries.len()
                );
            }
        }
    }

    Ok(())
}
