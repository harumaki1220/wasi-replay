use anyhow::Result;
use wit_parser::WorldItem;
use wit_parser::decoding::{DecodedWasm, decode};
use wit_parser::{Resolve, Type, TypeDefKind};

/// 型を人間が読める文字列にする
fn type_name(resolve: &Resolve, ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::S8 => "s8".into(),
        Type::S16 => "s16".into(),
        Type::S32 => "s32".into(),
        Type::S64 => "s64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Char => "char".into(),
        Type::String => "string".into(),
        Type::ErrorContext => "error-context".into(),
        Type::Id(id) => {
            let def = &resolve.types[*id];
            // 名前が付いていればそれを使う
            if let Some(name) = &def.name {
                return name.clone();
            }
            // 無名の型は構造で表す
            match &def.kind {
                TypeDefKind::List(inner) => format!("list<{}>", type_name(resolve, inner)),
                TypeDefKind::Option(inner) => format!("option<{}>", type_name(resolve, inner)),
                TypeDefKind::Tuple(t) => {
                    let items: Vec<_> = t.types.iter().map(|x| type_name(resolve, x)).collect();
                    format!("tuple<{}>", items.join(", "))
                }
                // Result と Handle は未対応。実際に扱うときに足す
                other => format!("{:?}", other),
            }
        }
    }
}

fn main() -> Result<()> {
    let bytes = std::fs::read("guest/target/wasm32-wasip1/release/guest.wasm")?;

    let DecodedWasm::Component(resolve, world_id) = decode(&bytes)? else {
        anyhow::bail!("コンポーネントではありませんでした");
    };

    let world = &resolve.worlds[world_id];

    for (key, item) in &world.imports {
        let name = resolve.name_world_key(key);
        match item {
            WorldItem::Interface { id, .. } => {
                let iface = &resolve.interfaces[*id];
                println!("interface {}", name);
                for (func_name, func) in &iface.functions {
                    let params: Vec<_> = func
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, type_name(&resolve, &p.ty)))
                        .collect();
                    let result = match &func.result {
                        Some(ty) => format!(" -> {}", type_name(&resolve, ty)),
                        None => String::new(),
                    };
                    println!("    {}({}){}", func_name, params.join(", "), result);
                }
            }
            WorldItem::Function(f) => println!("function {} : {:?}", name, f.params),
            WorldItem::Type { .. } => println!("type {}", name),
        }
    }
    Ok(())
}
