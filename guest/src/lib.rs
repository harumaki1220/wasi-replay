#[allow(warnings)]
mod bindings;

use bindings::wasi::clocks::wall_clock::now;
use bindings::wasi::random::random::get_random_u64;
use bindings::Guest;

struct Component;

impl Guest for Component {
    /// Say hello!
    fn hello_world() -> String {
        let a = get_random_u64();
        let t = now();
        let b = get_random_u64();
        format!("Hello, World! {} {}s {}", a, t.seconds, b)
    }
}

bindings::export!(Component with_types_in bindings);
