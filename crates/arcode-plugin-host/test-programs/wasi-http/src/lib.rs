//! `wasi-http` fixture: a component that imports `wasi:http`. The host must
//! refuse it at load time (§4: `wasi:http` is never linked).

wit_bindgen::generate!({
    path: "wit",
    world: "fixture",
    generate_all,
});

struct Fixture;

impl Guest for Fixture {
    fn run() -> u32 {
        wasi::http::outgoing_handler::handle(1)
    }
}

export!(Fixture);
