#[allow(warnings)]
mod bindings;

use bindings::wasi::random::random::get_random_u64;
use bindings::Guest;

struct Component;

impl Guest for Component {
    /// Say hello!
    fn hello_world() -> String {
        let n = get_random_u64();
        format!("Hello, World! {}", n)
    }
}

bindings::export!(Component with_types_in bindings);
